//! NFR5.HD1.app-host.DD1: every scope root is one item, counted once.
//!
//! A root is counted when the collector admits it, before its first read:
//!
//! | Root | Counted at admission | Final count | Gap at the root |
//! | --- | --- | --- | --- |
//! | read to its end | completed | completed | no |
//! | repeating an earlier root (revisit) | completed | completed | no |
//! | identity not verified | failed | failed | unverifiable |
//! | identity not verified, then a failed read | failed | failed | unverifiable and the read |
//! | first read failed | completed | failed, no result | read |
//! | later read failed or disconnected | completed | failed | read |
//! | volume closed before the root started | completed | failed | disconnected |
//! | cancelled after admission | completed | completed | no |
//! | cancelled before admission | not counted | not counted | no |
//!
//! The sequential collector and every parallel variant give the same counts.
//! A cancelled root that was admitted stays completed and is also reported in
//! `unprocessed_roots`, which counts unfinished roots, not unread items.
use super::*;
use du_core_contracts::scan_observation::{VolumeSpace, VolumeSpaceReader};

fn counts(collected: &Collected) -> (u64, u64) {
    let counts = &collected.run.outcome.counts;
    (counts.completed, counts.failed)
}

fn read_root() -> HashMap<u64, Listing> {
    HashMap::from([(ROOT, Listing::Entries(Vec::new()))])
}

fn unverified_root() -> HashMap<u64, Listing> {
    HashMap::from([
        (ROOT, Listing::Entries(vec![file("f.txt", 150)])),
        (UNVERIFIED, Listing::Entries(Vec::new())),
    ])
}

fn unverified_root_fails_later() -> HashMap<u64, Listing> {
    HashMap::from([
        (
            ROOT,
            Listing::FailsAfter(vec![file("f.txt", 150)], denied()),
        ),
        (UNVERIFIED, Listing::Entries(Vec::new())),
    ])
}

fn root_disconnects_later() -> HashMap<u64, Listing> {
    let batches = vec![vec![file("f.txt", 150)]];
    HashMap::from([(ROOT, Listing::Batches(batches, Some(disconnected())))])
}

#[test]
fn each_single_root_case_is_counted_once_by_both_collectors() -> Result<(), String> {
    let _guard = worker_test_lock();
    let read = compare(read_root, false)?;
    assert_eq!(counts(&read), (1, 0));
    assert_eq!(read.run.completeness, ResultCompleteness::Complete);

    let unverified = compare(unverified_root, false)?;
    assert_eq!(counts(&unverified), (1, 1));
    assert_eq!(unverified.run.summary.2, [unverifiable_gap(0, &[])]);

    // Two gap rows at the root path, one failure.
    let failed_later = compare(unverified_root_fails_later, false)?;
    assert_eq!(counts(&failed_later), (1, 1));
    let reasons: Vec<_> = failed_later
        .run
        .summary
        .2
        .iter()
        .map(|gap| gap.2.as_str())
        .collect();
    assert_eq!(
        reasons,
        [unentered_reason::BOUNDARY_UNVERIFIABLE, "scan.path_denied"]
    );

    let disconnected_later = compare(root_disconnects_later, false)?;
    assert_eq!(counts(&disconnected_later), (1, 1));
    assert_eq!(
        disconnected_later.run.summary.2,
        [gap(&[], "scan.disconnected")]
    );

    // Admitted, then cancelled: the root stays completed and unprocessed.
    let cancelled = compare(cancelled_root, true)?;
    assert_eq!(counts(&cancelled), (4, 1));
    assert_eq!(
        cancelled.run.outcome.unprocessed_summary.as_deref(),
        Some("unprocessed_roots=1")
    );
    Ok(())
}

#[test]
fn a_root_cancelled_before_admission_is_not_counted() -> Result<(), String> {
    let _guard = worker_test_lock();
    let ids = Ids(AtomicUsize::new(1));
    let sink = sink_with_result(&ids)?;
    let (_, root) = FaultDir::root(read_root());
    let outcome = crate::scan::collect(
        &sink,
        ids.next_id(),
        root,
        &ids,
        || Checkpoint::Cancel,
        |_| true,
    );
    let sequential = pinned(outcome, &sink)?;
    for workers in WORKERS {
        let ids = Ids(AtomicUsize::new(1));
        let sink = sink_with_result(&ids)?;
        let (_, root) = FaultDir::root(read_root());
        let context = Cancelling {
            flag: Some(Arc::new(AtomicBool::new(true))),
            inner: context(),
        };
        let build = sink.open_forest(ids.next_id(), vec![observed_root("C:\\R0", 1)]);
        let devices = DevicePlan {
            groups: vec![0],
            limit: 4,
        };
        let outcome = collect_open_forest_with_groups(
            build,
            vec![root],
            &ids,
            &context,
            workers,
            &|_| true,
            devices,
        );
        let parallel = pinned(outcome, &sink)?;
        for (label, run) in [("sequential", &sequential), ("parallel", &parallel)] {
            assert_eq!(counts(run), (0, 0), "{label} workers={workers}");
            assert_eq!(run.run.summary.0.len(), 1, "{label}: only the root item");
            assert_eq!(run.run.completeness, ResultCompleteness::Cancelled);
            assert_eq!(
                run.run.outcome.unprocessed_summary.as_deref(),
                Some("unprocessed_roots=1"),
                "{label}"
            );
        }
    }
    Ok(())
}

/// A root that lists one batch and, optionally, reports its volume as
/// disconnected when the scan starts.
struct Root {
    inode: u64,
    verified: bool,
    batch: Option<Vec<ObservedEntry>>,
    start_disconnected: bool,
}

impl Root {
    fn new(inode: u64) -> Self {
        Self {
            inode,
            verified: true,
            batch: Some(vec![file("f.txt", inode + 1)]),
            start_disconnected: false,
        }
    }
}

impl DirectoryObservations for Root {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self.batch.take().unwrap_or_default())
    }
}

struct DisconnectedVolume;

impl VolumeSpaceReader for DisconnectedVolume {
    fn read(&self, _: &[u8]) -> Result<VolumeSpace, PortError> {
        Err(disconnected())
    }
}

impl AnchoredDirectory for Root {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(denied())
    }

    fn observed_identity(&self) -> Option<HandleIdentity> {
        self.verified.then(|| HandleIdentity {
            device: 1,
            inode: self.inode,
            ancestors: Vec::new(),
        })
    }

    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        if self.start_disconnected {
            Ok(Box::new(DisconnectedVolume))
        } else {
            Err(error(PortErrorKind::Unsupported, "os.unsupported"))
        }
    }
}

/// Collects `roots`, all on volume 1 and in one device group.
fn forest(roots: Vec<Root>, workers: usize) -> Result<Collected, String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let observed = (0..roots.len())
        .map(|index| observed_root(&format!("C:\\R{index}"), 1))
        .collect();
    let build = sink.open_forest(ids.next_id(), observed);
    let devices = DevicePlan {
        groups: vec![0; roots.len()],
        limit: 4,
    };
    let roots = roots
        .into_iter()
        .map(|root| Box::new(root) as Box<dyn AnchoredDirectory>)
        .collect();
    let context = context();
    let outcome =
        collect_open_forest_with_groups(build, roots, &ids, &context, workers, &|_| true, devices);
    pinned(outcome, &sink)
}

#[test]
fn a_revisited_root_is_completed_without_a_gap() -> Result<(), String> {
    let _guard = worker_test_lock();
    for workers in WORKERS {
        let run = forest(vec![Root::new(7), Root::new(7)], workers)?;
        assert_counted_once(&run, "revisit");
        // Both roots and the first root's file are completed.
        assert_eq!(counts(&run), (3, 0), "workers={workers}");
        assert!(run.run.summary.1.contains(&revisit(1, &[])));
        assert!(run.run.summary.2.is_empty());
    }
    Ok(())
}

#[test]
fn roots_on_a_volume_closed_before_they_start_are_each_one_failure() -> Result<(), String> {
    let _guard = worker_test_lock();
    for workers in WORKERS {
        let mut first = Root::new(7);
        first.start_disconnected = true;
        let mut unverified = Root::new(9);
        unverified.verified = false;
        let run = forest(vec![first, unverified], workers)?;
        assert_counted_once(&run, "closed");
        assert_eq!(counts(&run), (0, 2), "workers={workers}");
        let gaps: Vec<_> = run
            .run
            .summary
            .2
            .iter()
            .map(|gap| (gap.0, gap.2.as_str()))
            .collect();
        assert_eq!(gaps, [(0, "scan.disconnected"), (1, "scan.disconnected")]);
        assert_eq!(run.run.summary.0.len(), 2, "no root was read");
    }
    Ok(())
}
