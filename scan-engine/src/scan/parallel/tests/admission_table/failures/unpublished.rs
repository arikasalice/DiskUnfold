//! NFR5.HD1.app-host.DD1: a collection that ends without a result counts no
//! item as completed.
//!
//! The runtime reports a job that did not finish its scope as partial when it
//! counted completed items, and a user reads partial as "some results exist".
//! When no result is published, both collectors move the items they counted
//! as completed to `unprocessed` and keep `failed`, so the job ends failed.
//!
//! | Case | Before | Now (completed, failed, unprocessed) |
//! | --- | --- | --- |
//! | first root read fails | (0, 1, 0) | (0, 1, 0) |
//! | root admitted, then publication fails | (1, 0, 0) | (0, 0, 1) |
//! | items counted, then publication fails | (3, 1, 0) | (0, 1, 3) |
use super::*;

/// Hands out a version ID the sink already used, so every publication fails.
struct Reused(Id);

impl ResultIds for Reused {
    fn next_id(&self) -> Id {
        self.0.clone()
    }
}

struct Case {
    label: &'static str,
    listings: fn() -> HashMap<u64, Listing>,
    /// Publishes with an already used version ID, which the store refuses.
    reuse: bool,
    failed: u64,
    unprocessed: u64,
}

fn first_read_fails() -> HashMap<u64, Listing> {
    HashMap::from([(ROOT, Listing::FailsAfter(Vec::new(), denied()))])
}

fn empty_root() -> HashMap<u64, Listing> {
    HashMap::from([(ROOT, Listing::Entries(Vec::new()))])
}

fn counted_entries() -> HashMap<u64, Listing> {
    let entries = vec![
        file("a.txt", 150),
        file("b.txt", 151),
        without_metadata("meta.bin", 152, denied()),
    ];
    HashMap::from([(ROOT, Listing::Entries(entries))])
}

const CASES: [Case; 3] = [
    Case {
        label: "first root read fails",
        listings: first_read_fails,
        reuse: false,
        failed: 1,
        unprocessed: 0,
    },
    Case {
        label: "root admitted, publication fails",
        listings: empty_root,
        reuse: true,
        failed: 0,
        unprocessed: 1,
    },
    Case {
        label: "items counted, publication fails",
        listings: counted_entries,
        reuse: true,
        failed: 1,
        unprocessed: 3,
    },
];

/// Runs the sequential collector (`None`) or a parallel variant given as
/// workers and filter mode against a sink that already holds a result.
/// Returns the outcome and whether that older result is still current.
fn run(case: &Case, parallel: Option<(usize, bool)>) -> Result<(JobOutcome, bool), String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = sink_with_result(&ids)?;
    let current = |sink: &IngestSink| -> Result<Id, String> {
        let version = sink.pin().map_err(|e| format!("{e:?}"))?;
        Ok(version.ok_or("missing result")?.version_id().clone())
    };
    let previous = current(&sink)?;
    let reused = Reused(previous.clone());
    let result_ids: &dyn ResultIds = if case.reuse { &reused } else { &ids };
    let (_, root) = FaultDir::root((case.listings)());
    let scope = ids.next_id();
    let outcome = match parallel {
        None => crate::scan::collect(
            &sink,
            scope,
            root,
            result_ids,
            || Checkpoint::Continue,
            |_| true,
        ),
        Some((workers, filtered)) => {
            let build = sink.open_forest(scope, vec![observed_root("C:\\R0", 1)]);
            let devices = DevicePlan {
                groups: vec![0],
                limit: 4,
            };
            let (roots, context) = (vec![root], context());
            if filtered {
                collect_open_forest_filtered(
                    build, roots, result_ids, &context, workers, &admit, devices,
                )
            } else {
                collect_open_forest_with_groups(
                    build,
                    roots,
                    result_ids,
                    &context,
                    workers,
                    &|_| true,
                    devices,
                )
            }
        }
    };
    Ok((outcome, current(&sink)? == previous))
}

#[test]
fn a_collection_without_a_result_counts_no_item_as_completed() -> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let parallel = WORKERS
        .iter()
        .flat_map(|&workers| [false, true].map(|filtered| Some((workers, filtered))));
    let collectors: Vec<_> = std::iter::once(None).chain(parallel).collect();
    for case in &CASES {
        for &collector in &collectors {
            let label = format!("{} {collector:?}", case.label);
            let (outcome, kept) = run(case, collector)?;
            assert_eq!(outcome.result_version_id, None, "{label}");
            assert!(!outcome.completed_scope, "{label}");
            assert_eq!(
                outcome.failure_reason_key.as_deref(),
                Some("support.internal_error"),
                "{label}"
            );
            let counts = &outcome.counts;
            assert_eq!(
                (counts.completed, counts.failed, counts.unprocessed),
                (0, case.failed, case.unprocessed),
                "{label}"
            );
            assert!(kept, "{label}: the older result stays current");
        }
    }
    Ok(())
}
