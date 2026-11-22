//! FR12.HD1.app-host.DD1 (DEC-006): one failing input gives one result.
//!
//! The fixture directories refuse opens, fail reads, return oversized batches
//! and report broken metadata. The sequential collector and the parallel
//! collector with 1, 2, 4 and 8 workers, with the legacy predicate and with
//! the pre-metadata filter, must record the same items, failure list, scope
//! gaps, counts and completeness. Disconnection and cancellation cases stop
//! at a point that does not depend on worker scheduling. Every result counts
//! each item once, the scope root included (NFR5.HD1.app-host.DD1): `failed`
//! equals the number of paths with a scope gap, and `completed + failed` the
//! number of items. A collection that publishes no result counts no item as
//! completed (see `unpublished`).
use super::*;
use du_core_contracts::result::SpaceAggregate;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
mod fixture;
mod nested;
mod root_counts;
mod spread;
mod unpublished;
use fixture::*;
mod cancel_disconnect;

const WORKERS: [usize; 4] = [1, 2, 4, 8];

/// Every failure a directory or an entry can report, beside clean rows.
fn every_failure() -> HashMap<u64, Listing> {
    let mut swapped = file("swapped", 206);
    swapped.enumerated_inode = 207;
    let mut sizeless = file("sizeless.txt", 208);
    if let Ok(meta) = &mut sizeless.metadata {
        meta.size = Err(denied());
    }
    let root = vec![
        dir("denied", 110),
        dir("gone", 111),
        dir("broken", 112),
        dir("partial", 113),
        dir("huge", 114),
        dir("ok", 115),
        dir("twin", 115),
        observed("mnt", ObservedKind::Directory, 2, 300),
        observed("away", ObservedKind::Link, 1, 201),
        without_metadata("meta.bin", 202, denied()),
        swapped,
        observed("unknown", ObservedKind::Unknown, 1, 203),
        observed("other", ObservedKind::Other, 1, 204),
        sizeless,
        file("top.txt", 205),
    ];
    let failed_read = error(PortErrorKind::Io, "os.query_failed");
    HashMap::from([
        (ROOT, Listing::Entries(root)),
        (110, Listing::Refused(denied())),
        (
            111,
            Listing::Refused(error(PortErrorKind::NotFound, "os.gone")),
        ),
        (112, Listing::FailsAfter(Vec::new(), denied())),
        (
            113,
            Listing::FailsAfter(vec![file("p.txt", 220)], failed_read),
        ),
        (114, Listing::Oversized),
        (115, Listing::Entries(vec![file("deep.txt", 221)])),
        (300, Listing::Entries(vec![file("other.txt", 301)])),
    ])
}

/// A collection with the root aggregate, which shows how gaps mark totals.
struct Collected {
    run: Run,
    root: Option<SpaceAggregate>,
    published: bool,
}

fn pinned(outcome: JobOutcome, sink: &IngestSink) -> Result<Collected, String> {
    let published = outcome.result_version_id.is_some();
    let version = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    Ok(Collected {
        root: version.space(0),
        run: Run {
            outcome,
            summary: summary(&version),
            completeness: version.completeness(),
        },
        published,
    })
}

/// A sink holding an older result, so a failed collection leaves it pinned.
fn sink_with_result(ids: &Ids) -> Result<IngestSink, String> {
    let sink = IngestSink::default();
    let mut build = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(ids.next_id(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok(sink)
}

fn sequential(listings: HashMap<u64, Listing>, cancel: bool) -> Result<Collected, String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = sink_with_result(&ids)?;
    let (faults, root) = FaultDir::root(listings);
    let checkpoint = || match cancel && faults.cancel.load(SeqCst) {
        true => Checkpoint::Cancel,
        false => Checkpoint::Continue,
    };
    let outcome = crate::scan::collect(&sink, ids.next_id(), root, &ids, checkpoint, |_| true);
    pinned(outcome, &sink)
}

/// Cancels once the root has returned its first batch, when asked to.
struct Cancelling {
    flag: Option<Arc<AtomicBool>>,
    inner: Context,
}

impl JobContext for Cancelling {
    fn checkpoint(&self) -> Checkpoint {
        match self.flag.as_ref().is_some_and(|flag| flag.load(SeqCst)) {
            true => Checkpoint::Cancel,
            false => Checkpoint::Continue,
        }
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        self.inner.try_start_flow()
    }
}

fn admit(_: u32, _: &[Vec<u8>], _: &[u8]) -> FilterDecision {
    FilterDecision::Admit
}

fn parallel(
    listings: HashMap<u64, Listing>,
    workers: usize,
    filtered: bool,
    cancel: bool,
) -> Result<(Collected, Arc<Faults>), String> {
    parallel_seeded(listings, (workers, 0), filtered, cancel)
}

/// Like [`parallel`], with the fixture's seed-dependent delays.
fn parallel_seeded(
    listings: HashMap<u64, Listing>,
    (workers, seed): (usize, u64),
    filtered: bool,
    cancel: bool,
) -> Result<(Collected, Arc<Faults>), String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = sink_with_result(&ids)?;
    let (faults, root) = FaultDir::seeded(listings, seed);
    let context = Cancelling {
        flag: cancel.then(|| Arc::clone(&faults.cancel)),
        inner: context(),
    };
    let build = sink.open_forest(ids.next_id(), vec![observed_root("C:\\R0", 1)]);
    let devices = DevicePlan {
        groups: vec![0],
        limit: 4,
    };
    let roots = vec![root];
    let outcome = if filtered {
        collect_open_forest_filtered(build, roots, &ids, &context, workers, &admit, devices)
    } else {
        collect_open_forest_with_groups(build, roots, &ids, &context, workers, &|_| true, devices)
    };
    Ok((pinned(outcome, &sink)?, faults))
}

/// Checks that a published result counted each of its items exactly once,
/// and that a collection without a result counted none as completed.
///
/// A root whose unverifiable start was counted and whose read then failed
/// keeps two gap rows at one path, so gaps are counted by path.
fn assert_counted_once(collected: &Collected, label: &str) {
    let counts = &collected.run.outcome.counts;
    if !collected.published {
        // The pinned summary is the older result, which this job did not build.
        assert_eq!(counts.completed, 0, "{label}: no result, nothing completed");
        return;
    }
    let (items, _, gaps) = &collected.run.summary;
    let paths: std::collections::BTreeSet<_> = gaps.iter().map(|gap| (gap.0, &gap.1)).collect();
    assert_eq!(
        counts.failed,
        paths.len() as u64,
        "{label}: failed = gap paths"
    );
    let items = items.len() as u64;
    assert_eq!(counts.completed + counts.failed, items, "{label}: once");
}

/// Runs every parallel variant and compares it with the sequential baseline.
fn compare(listings: fn() -> HashMap<u64, Listing>, cancel: bool) -> Result<Collected, String> {
    let baseline = sequential(listings(), cancel)?;
    assert_counted_once(&baseline, "sequential");
    for workers in WORKERS {
        for filtered in [false, true] {
            let label = format!("workers={workers} filtered={filtered}");
            let (run, _) = parallel(listings(), workers, filtered, cancel)?;
            let (expected, actual) = (&baseline.run, &run.run);
            assert_eq!(actual.summary, expected.summary, "{label}");
            assert_eq!(actual.completeness, expected.completeness, "{label}");
            assert_eq!(run.root, baseline.root, "{label}");
            assert_eq!(run.published, baseline.published, "{label}");
            let (expected, actual) = (&expected.outcome, &actual.outcome);
            assert_eq!(actual.counts.failed, expected.counts.failed, "{label}");
            assert_eq!(
                actual.counts.completed, expected.counts.completed,
                "{label}"
            );
            assert_eq!(actual.completed_scope, expected.completed_scope, "{label}");
            assert_eq!(
                actual.failure_reason_key, expected.failure_reason_key,
                "{label}"
            );
            assert_eq!(
                actual.unprocessed_summary, expected.unprocessed_summary,
                "{label}"
            );
        }
    }
    Ok(baseline)
}

fn range(parts: &[&str], stage: Stage, reason: &str) -> Issue {
    (0, path(parts), Kind::UnreadRange, stage, reason.to_string())
}

fn entry_issue(parts: &[&str], kind: Kind, reason: &str) -> Issue {
    (0, path(parts), kind, Stage::Metadata, reason.to_string())
}

fn gap(parts: &[&str], reason: &str) -> Gap {
    (0, path(parts), reason.to_string())
}

#[test]
fn every_failure_kind_gives_one_result_from_both_collectors() -> Result<(), String> {
    let _guard = worker_test_lock();
    let baseline = compare(every_failure, false)?;
    let (denied, vanished, other) = (
        "scan.path_denied",
        "scan.path_vanished",
        "scan.unclassified",
    );
    let (replaced, unverifiable) = ("scan.item_replaced", "scan.item_unverifiable");
    let mut issues = vec![
        range(&["denied"], Stage::OpenDirectory, denied),
        range(&["gone"], Stage::OpenDirectory, vanished),
        range(&["broken"], Stage::Enumerate, denied),
        range(&["partial"], Stage::Enumerate, other),
        range(&["huge"], Stage::Enumerate, other),
        entry_issue(&["meta.bin"], Kind::CollectionFailed, denied),
        entry_issue(&["swapped"], Kind::Uncertain, replaced),
        entry_issue(&["unknown"], Kind::Uncertain, unverifiable),
        entry_issue(&["other"], Kind::Uncertain, unverifiable),
        entry_issue(&["sizeless.txt"], Kind::CollectionFailed, denied),
        link(0, &["away"]),
        other_volume(0, &["mnt"]),
        revisit(0, &["twin"]),
    ];
    let mut listed = baseline.run.summary.1.clone();
    issues.sort_by(|a, b| (&a.1, a.2).cmp(&(&b.1, b.2)));
    listed.sort_by(|a, b| (&a.1, a.2).cmp(&(&b.1, b.2)));
    assert_eq!(listed, issues);
    let mut gaps = vec![
        gap(&["denied"], denied),
        gap(&["gone"], vanished),
        gap(&["broken"], denied),
        gap(&["partial"], other),
        gap(&["huge"], other),
        gap(&["meta.bin"], denied),
        gap(&["swapped"], replaced),
        gap(&["unknown"], unverifiable),
        gap(&["other"], unverifiable),
        gap(&["sizeless.txt"], denied),
    ];
    gaps.sort();
    assert_eq!(baseline.run.summary.2, gaps);
    assert_eq!(baseline.run.outcome.counts.failed, 10);
    // Each failed entry or directory counts once, as failed; the unknown and
    // other kinds, the file without a size and the five unread directories
    // are not completed as well. The root read without a gap is completed.
    assert_eq!(baseline.run.outcome.counts.completed, 8);
    assert_eq!(baseline.run.completeness, ResultCompleteness::Missing);
    let root = baseline.root.ok_or("missing root aggregate")?;
    assert_eq!(root.completeness, ResultCompleteness::Missing);
    // The entries read before the failed read stay in the result.
    assert!(
        baseline
            .run
            .summary
            .0
            .iter()
            .any(|item| item.1 == path(&["partial", "p.txt"]))
    );
    Ok(())
}

#[test]
fn a_failed_first_root_read_leaves_no_result() -> Result<(), String> {
    let _guard = worker_test_lock();
    for listing in [
        Listing::FailsAfter(Vec::new(), denied()),
        Listing::Oversized,
    ] {
        let listings = HashMap::from([(ROOT, listing)]);
        let baseline = sequential(listings.clone(), false)?;
        assert!(!baseline.published);
        assert_eq!(
            baseline.run.outcome.failure_reason_key.as_deref(),
            Some("support.internal_error")
        );
        assert_eq!(baseline.run.outcome.counts.failed, 1);
        // The root was admitted as completed and its failed read moved it.
        assert_eq!(baseline.run.outcome.counts.completed, 0);
        // The older result stays pinned.
        assert_eq!(baseline.run.completeness, ResultCompleteness::Complete);
        for workers in WORKERS {
            let (run, _) = parallel(listings.clone(), workers, false, false)?;
            assert!(!run.published, "workers={workers}");
            assert_eq!(run.run.outcome.counts.failed, 1, "workers={workers}");
            assert_eq!(run.run.outcome.counts.completed, 0, "workers={workers}");
            assert_eq!(run.run.summary, baseline.run.summary, "workers={workers}");
        }
    }
    Ok(())
}

fn root_batch_disconnects() -> HashMap<u64, Listing> {
    let entries = vec![
        dir("folder", 120),
        file("held", 121),
        without_metadata("lost", 122, disconnected()),
    ];
    HashMap::from([
        (ROOT, Listing::Entries(entries)),
        (120, Listing::Entries(vec![file("inner.txt", 123)])),
    ])
}

fn root_read_disconnects() -> HashMap<u64, Listing> {
    HashMap::from([(ROOT, Listing::FailsAfter(Vec::new(), disconnected()))])
}

#[test]
fn a_disconnected_root_is_a_gap_in_both_collectors() -> Result<(), String> {
    let _guard = worker_test_lock();
    let reason = "scan.disconnected";
    let baseline = compare(root_batch_disconnects, false)?;
    // The returned directory is not opened, so it is a gap of its own.
    assert_eq!(
        baseline.run.summary.2,
        [
            gap(&[], reason),
            gap(&["folder"], reason),
            gap(&["lost"], reason)
        ]
    );
    let issues = [
        range(&[], Stage::Enumerate, reason),
        range(&["folder"], Stage::Enumerate, reason),
        entry_issue(&["lost"], Kind::CollectionFailed, reason),
    ];
    assert_eq!(baseline.run.summary.1, issues);
    let names: Vec<_> = baseline
        .run
        .summary
        .0
        .iter()
        .map(|item| item.1.clone())
        .collect();
    assert_eq!(
        names,
        [
            path(&[]),
            path(&["folder"]),
            path(&["held"]),
            path(&["lost"])
        ]
    );
    assert_eq!(baseline.run.outcome.counts.failed, 3);
    assert_eq!(baseline.run.outcome.counts.completed, 1);
    assert!(baseline.run.outcome.completed_scope);
    assert_eq!(baseline.run.completeness, ResultCompleteness::Missing);
    let (_, faults) = parallel(root_batch_disconnects(), 4, false, false)?;
    assert!(!faults.opened(120));
    let baseline = compare(root_read_disconnects, false)?;
    assert!(baseline.published);
    assert_eq!(
        baseline.run.summary.1,
        [range(&[], Stage::Enumerate, reason)]
    );
    assert_eq!(baseline.run.summary.2, [gap(&[], reason)]);
    assert_eq!(baseline.run.completeness, ResultCompleteness::Missing);
    Ok(())
}

fn cancelled_root() -> HashMap<u64, Listing> {
    let entries = vec![
        dir("a", 101),
        observed("away", ObservedKind::Link, 1, 201),
        without_metadata("meta.bin", 202, denied()),
        file("top.txt", 205),
    ];
    HashMap::from([
        (ROOT, Listing::Entries(entries)),
        (101, Listing::Entries(vec![file("deep.txt", 221)])),
    ])
}

#[test]
fn cancellation_records_returned_entries_without_opening_them() -> Result<(), String> {
    let _guard = worker_test_lock();
    let baseline = compare(cancelled_root, true)?;
    assert_eq!(baseline.run.completeness, ResultCompleteness::Cancelled);
    assert!(!baseline.run.outcome.completed_scope);
    assert_eq!(
        baseline.run.outcome.unprocessed_summary.as_deref(),
        Some("unprocessed_roots=1")
    );
    assert_eq!(baseline.run.summary.0.len(), 5);
    let denied = "scan.path_denied";
    let issues = [
        link(0, &["away"]),
        entry_issue(&["meta.bin"], Kind::CollectionFailed, denied),
    ];
    assert_eq!(baseline.run.summary.1, issues);
    assert_eq!(baseline.run.summary.2, [gap(&["meta.bin"], denied)]);
    for workers in WORKERS {
        let (_, faults) = parallel(cancelled_root(), workers, false, true)?;
        assert!(!faults.opened(101), "workers={workers}");
    }
    Ok(())
}
