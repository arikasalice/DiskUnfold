use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id, SizeAmount,
    job::Checkpoint,
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, MetadataObservation, ObservedEntry, ObservedKind,
        SpaceIdentity,
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
struct Directory {
    entries: Option<Vec<ObservedEntry>>,
    fail: bool,
}
impl DirectoryObservations for Directory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if self.fail {
            return Err(error());
        }
        Ok(self.entries.take().unwrap_or_default())
    }
}
impl AnchoredDirectory for Directory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        if entry.name == b"denied" {
            return Err(error());
        }
        Ok(Box::new(Directory {
            entries: Some(vec![file(b"child", 3)]),
            fail: false,
        }))
    }
}
fn error() -> PortError {
    PortError {
        kind: PortErrorKind::PermissionDenied,
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
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
        }),
    }
}
fn file(name: &[u8], size: u64) -> ObservedEntry {
    entry(name, ObservedKind::File, Ok(size))
}

#[test]
fn disconnected_metadata_stays_a_failure_not_zero_or_general_io()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ids = ids();
    let disconnected = PortError {
        kind: PortErrorKind::Io,
        reason_key: "scan.disconnected".into(),
        os_code: Some(1167),
    };
    let unknown = PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: Some(1117),
    };
    let root = Directory {
        entries: Some(vec![
            entry(b"removed", ObservedKind::File, Err(disconnected)),
            entry(b"io", ObservedKind::File, Err(unknown)),
        ]),
        fail: false,
    };
    let result = collect(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    assert_eq!(result.counts.failed, 2);
    let version = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    let issues = version.scan_issues();
    assert!(issues.iter().any(|issue| issue.relative[0] == b"removed" && issue.reason_key == "scan.disconnected"));
    assert!(
        issues
            .iter()
            .any(|issue| issue.relative[0] == b"io" && issue.reason_key == "scan.unclassified")
    );
    assert_eq!(version.completeness(), ResultCompleteness::Missing);
    assert!(
        version
            .items()
            .iter()
            .filter(|item| item.name == b"removed" || item.name == b"io")
            .all(|item| item.size.is_none())
    );
    Ok(())
}
fn ids() -> Ids {
    Ids(AtomicUsize::new(1))
}

#[test]
fn publishes_tree_and_records_failures_without_zeroing() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ids = ids();
    let root = Directory {
        entries: Some(vec![
            file(b"zero", 0),
            entry(b"denied", ObservedKind::Directory, Ok(0)),
            entry(b"link", ObservedKind::Link, Ok(0)),
            entry(b"unknown", ObservedKind::File, Err(error())),
        ]),
        fail: false,
    };
    let outcome = collect(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    assert_eq!(outcome.counts.failed, 2);
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("no result")?;
    use du_core_contracts::scan_failures::{ScanFailureKind as Kind, ScanFailureStage as Stage};
    let facts = result.scan_issues();
    assert_eq!(facts.len(), 3);
    assert_eq!(
        (
            facts[0].relative[0].as_slice(),
            facts[0].kind,
            facts[0].stage
        ),
        (
            b"denied".as_slice(),
            Kind::UnreadRange,
            Stage::OpenDirectory
        )
    );
    assert_eq!(
        (
            facts[1].relative[0].as_slice(),
            facts[1].kind,
            facts[1].stage,
            facts[1].reason_key.as_str()
        ),
        (
            b"link".as_slice(),
            Kind::UntraversedLink,
            Stage::Traverse,
            "scan.link_untraversed"
        )
    );
    assert_eq!(
        (
            facts[2].relative[0].as_slice(),
            facts[2].kind,
            facts[2].stage
        ),
        (
            b"unknown".as_slice(),
            Kind::CollectionFailed,
            Stage::Metadata
        )
    );
    assert!(facts.iter().all(|fact| fact.observed_at.is_none()));
    assert!(result.scope_gaps().is_empty());
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    assert_eq!(result.items().len(), 5);
    assert_eq!(result.items()[0].kind, ResultItemKind::Directory);
    let totals = result.space(0).ok_or("missing totals")?;
    assert_eq!(totals.missing_items, 1);
    assert_eq!(totals.untraversed_links, 1);
    assert!(
        result
            .items()
            .iter()
            .find(|i| i.name == b"unknown")
            .ok_or("missing")?
            .size
            .is_none()
    );
    Ok(())
}

#[test]
fn cancellation_seals_and_root_failure_aborts() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ids = ids();
    let root = Directory {
        entries: Some(vec![file(b"a", 1)]),
        fail: false,
    };
    let result = collect(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        || Checkpoint::Cancel,
        |_| true,
    );
    assert!(!result.completed_scope);
    assert_eq!(
        sink.pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing")?
            .completeness(),
        ResultCompleteness::Cancelled
    );
    let root = Directory {
        entries: None,
        fail: true,
    };
    let result = collect(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    assert!(result.result_version_id.is_none());
    assert_eq!(
        sink.pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing")?
            .completeness(),
        ResultCompleteness::Cancelled
    );
    Ok(())
}

#[test]
fn replacement_and_excluded_entries_are_not_opened() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ids = ids();
    let mut replaced = entry(b"changed", ObservedKind::Directory, Ok(0));
    replaced.enumerated_inode = 9;
    let root = Directory {
        entries: Some(vec![
            replaced,
            entry(b"skip", ObservedKind::Directory, Ok(0)),
        ]),
        fail: false,
    };
    let outcome = collect(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        || Checkpoint::Continue,
        |path| path.last().is_none_or(|n| n != b"skip"),
    );
    assert_eq!(outcome.counts.failed, 1);
    let result = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    assert_eq!(result.items().len(), 2);
    assert_eq!(result.items()[1].name, b"changed");
    assert_eq!(
        result.scan_issues()[0].kind,
        du_core_contracts::scan_failures::ScanFailureKind::Uncertain
    );
    assert!(result.items()[1].size.is_none());
    Ok(())
}

struct EmptyRoot(bool);

impl DirectoryObservations for EmptyRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if std::mem::replace(&mut self.0, true) {
            return Err(error());
        }
        Ok(Vec::new())
    }
}

impl AnchoredDirectory for EmptyRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(error())
    }
}

#[test]
fn collection_stops_at_the_first_end_of_directory() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ids = ids();
    let outcome = collect(
        &sink,
        ids.next_id(),
        Box::new(EmptyRoot(false)),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("no result")?;
    assert_eq!(result.completeness(), ResultCompleteness::Complete);
    assert_eq!(outcome.counts.failed, 0);
    assert_eq!(result.items().len(), 1);
    Ok(())
}

#[test]
fn allocation_deduplicates_hard_links_without_assuming_unshared_extents()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ids = ids();
    let mut first = file(b"first", 10);
    if let Ok(m) = &mut first.metadata {
        m.allocated = Some(4096);
        m.link_count = Some(2);
        m.space_identity = Some(SpaceIdentity {
            volume: vec![1],
            file: vec![2],
        });
    }
    let mut second = first.clone();
    second.name = b"second".to_vec();
    let outcome = collect(
        &sink,
        ids.next_id(),
        Box::new(Directory {
            entries: Some(vec![first, second]),
            fail: false,
        }),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    assert_eq!(outcome.counts.failed, 0);
    let result = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    let aggregate = result.space(0).ok_or("missing aggregate")?;
    assert_eq!(
        aggregate.logical.amount,
        SizeAmount::Exact {
            bytes: 20u64.into()
        }
    );
    assert_eq!(
        aggregate.allocated.amount,
        SizeAmount::Exact {
            bytes: 4096u64.into()
        }
    );
    assert_eq!(aggregate.unique_physical.amount, SizeAmount::Unknown);
    Ok(())
}

#[test]
fn missing_allocation_is_not_zero_or_exact() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ids = ids();
    let root = Directory {
        entries: Some(vec![file(b"unknown", 7)]),
        fail: false,
    };
    collect(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        || Checkpoint::Continue,
        |_| true,
    );
    let result = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    let aggregate = result.space(0).ok_or("missing aggregate")?;
    assert_eq!(
        aggregate.logical.amount,
        SizeAmount::Exact { bytes: 7u64.into() }
    );
    assert_eq!(aggregate.allocated.amount, SizeAmount::Unknown);
    assert_eq!(aggregate.unique_physical.amount, SizeAmount::Unknown);
    Ok(())
}
