//! #149, F03 FR11.HD1: a scope root whose volume cannot share extents between
//! files confirms that no file below it shares any, so the unique physical
//! size is known. Any other root leaves it unknown.
use du_analysis_index::IngestSink;
use du_core_contracts::scan_observation::ObservedValue;
use du_core_contracts::{
    Id, SizeAmount,
    job::Checkpoint,
    ports::{PortError, PortErrorKind},
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

/// A root with only files, on a volume that can or cannot share extents.
struct Root {
    entries: Option<Vec<ObservedEntry>>,
    unshared: bool,
}
impl DirectoryObservations for Root {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self.entries.take().unwrap_or_default())
    }
}
impl AnchoredDirectory for Root {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
    fn observed_identity(&self) -> Option<HandleIdentity> {
        Some(HandleIdentity {
            device: 1,
            inode: u64::MAX,
            ancestors: Vec::new(),
        })
    }
    fn creates_no_shared_extents(&self) -> bool {
        self.unshared
    }
}

/// A file with its allocation, link count and identity observed.
fn file(name: &[u8], allocated: u64, links: u64, id: u8) -> ObservedEntry {
    ObservedEntry {
        name: name.to_vec(),
        enumerated_inode: u64::from(id),
        enumerated_kind: ObservedKind::File,
        metadata: Ok(MetadataObservation {
            kind: ObservedKind::File,
            device: 1,
            inode: u64::from(id),
            size: Ok(allocated / 2),
            allocated: ObservedValue::Known(allocated),
            link_count: ObservedValue::Known(links),
            space_identity: ObservedValue::Known(SpaceIdentity {
                volume: vec![1],
                file: vec![id],
            }),
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
        }),
    }
}

/// The allocated and unique physical totals of a root holding a file with two
/// links and another file.
fn totals(unshared: bool) -> Result<(SizeAmount, SizeAmount), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ids = Ids(AtomicUsize::new(0));
    let root = Root {
        entries: Some(vec![
            file(b"first", 4096, 2, 2),
            file(b"second", 4096, 2, 2),
            file(b"other", 8192, 1, 3),
        ]),
        unshared,
    };
    let outcome = collect(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    assert_eq!(outcome.counts.failed, 0);
    let result = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    let aggregate = result.space(0).ok_or("missing aggregate")?;
    Ok((
        aggregate.allocated.amount.clone(),
        aggregate.unique_physical.amount.clone(),
    ))
}

#[test]
fn a_volume_without_shared_extents_confirms_the_unique_physical_size()
-> Result<(), Box<dyn std::error::Error>> {
    // The file with two links is counted once.
    let both = SizeAmount::Exact {
        bytes: 12_288u64.into(),
    };
    assert_eq!(totals(true)?, (both.clone(), both));
    Ok(())
}

#[test]
fn a_volume_that_may_share_extents_leaves_the_unique_physical_size_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    let (allocated, unique) = totals(false)?;
    assert_eq!(
        allocated,
        SizeAmount::Exact {
            bytes: 12_288u64.into()
        }
    );
    assert_eq!(unique, SizeAmount::Unknown);
    Ok(())
}
