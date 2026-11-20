use du_analysis_index::IngestSink;
use du_core_contracts::scan_observation::ObservedValue;
use du_core_contracts::{
    Id, SizeAmount,
    job::Checkpoint,
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, HandleIdentity, MetadataObservation,
        ObservedEntry, ObservedKind, SpaceIdentity,
    },
};
use du_scan_engine::scan::{ResultIds, collect};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Ids(AtomicUsize);

impl ResultIds for Ids {
    fn next_id(&self) -> Id {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap_or_else(|_| unreachable!())
    }
}

struct Directory(Option<Vec<ObservedEntry>>);

impl DirectoryObservations for Directory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self.0.take().unwrap_or_default())
    }
}

impl AnchoredDirectory for Directory {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(error())
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        Some(HandleIdentity {
            device: 1,
            inode: u64::MAX,
            ancestors: Vec::new(),
        })
    }
}

fn ids() -> Ids {
    Ids(AtomicUsize::new(1))
}

fn error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

fn entry(name: &[u8], kind: ObservedKind, size: Result<u64, PortError>) -> ObservedEntry {
    ObservedEntry {
        name: name.to_vec(),
        enumerated_inode: 1,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device: 1,
            inode: 1,
            size,
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            space_identity: ObservedValue::NotApplicable,
        }),
    }
}

fn file(name: &[u8], size: u64) -> ObservedEntry {
    entry(name, ObservedKind::File, Ok(size))
}

#[test]
fn untraversed_links_keep_their_observed_size_and_space() -> Result<(), Box<dyn std::error::Error>>
{
    let sink = IngestSink::default();
    let ids = ids();
    let mut first = entry(b"first", ObservedKind::Link, Ok(7));
    if let Ok(meta) = &mut first.metadata {
        meta.allocated = ObservedValue::Known(4096);
        meta.link_count = ObservedValue::Known(2);
        meta.space_identity = ObservedValue::Known(SpaceIdentity {
            volume: vec![1],
            file: vec![2],
        });
    }
    let mut second = first.clone();
    second.name = b"second".to_vec();

    let outcome = collect(
        &sink,
        ids.next_id(),
        Box::new(Directory(Some(vec![first, second]))),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    assert_eq!(outcome.counts.failed, 0);
    let result = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    assert_eq!(result.completeness(), ResultCompleteness::Complete);
    let aggregate = result.space(0).ok_or("missing aggregate")?;
    assert_eq!(
        aggregate.logical.amount,
        SizeAmount::Exact {
            bytes: 14u64.into()
        }
    );
    assert_eq!(
        aggregate.allocated.amount,
        SizeAmount::Exact {
            bytes: 4096u64.into()
        }
    );
    assert_eq!(aggregate.unique_physical.amount, SizeAmount::Unknown);
    assert_eq!(aggregate.untraversed_links, 2);
    Ok(())
}

#[test]
fn an_untraversed_link_without_size_keeps_the_total_as_a_lower_bound()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ids = ids();
    let outcome = collect(
        &sink,
        ids.next_id(),
        Box::new(Directory(Some(vec![
            file(b"known", 7),
            entry(b"unsized-link", ObservedKind::Link, Err(error())),
        ]))),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    assert_eq!(outcome.counts.failed, 0);
    let result = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    assert_eq!(result.completeness(), ResultCompleteness::Complete);
    let link = result
        .items()
        .iter()
        .find(|item| item.name == b"unsized-link")
        .ok_or("missing link")?;
    assert_eq!(link.kind, ResultItemKind::UntraversedLink);
    assert!(link.size.is_none());
    let aggregate = result.space(0).ok_or("missing aggregate")?;
    assert_eq!(
        aggregate.logical.amount,
        SizeAmount::LowerBound { bytes: 7u64.into() }
    );
    assert_eq!(aggregate.untraversed_links, 1);
    Ok(())
}
