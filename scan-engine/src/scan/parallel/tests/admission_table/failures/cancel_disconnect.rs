//! Cancellation after a disconnection, between port calls.
//!
//! FR12.HD1.app-host.DD1 reads a folder whose enumeration had started to its
//! end after its volume disconnected. A cancellation stops that reading at
//! the next call: no read or open starts once it was requested, apart from a
//! call another worker was already issuing when it arrived. A call that is
//! already waiting in the adapter cannot be interrupted and returns first.
use super::*;

/// The root lists three batches; its child `c` reports the disconnection, so
/// the sequential collector closes the volume with the root still started.
fn root_beside_a_disconnect() -> HashMap<u64, Listing> {
    let root = vec![
        vec![dir("c", 401)],
        vec![file("x.txt", 402)],
        vec![file("y.txt", 403)],
    ];
    HashMap::from([
        (ROOT, Listing::Batches(root, None)),
        (401, Listing::Entries(disconnecting())),
        (405, Listing::Entries(vec![file("never.txt", 406)])),
    ])
}

/// `c`'s batch reports the disconnection beside a folder that is never opened.
fn disconnecting() -> Vec<ObservedEntry> {
    vec![without_metadata("lost", 404, disconnected()), dir("d", 405)]
}

/// `s` lists three batches beside `c`, which reports the disconnection.
///
/// With `gated`, the later reads of `s` wait until the disconnection was
/// reported, so a read of `s` is in flight when its volume closes. Another
/// worker must then read `c`, so gating needs two workers.
fn sibling_beside_a_disconnect(gated: bool) -> HashMap<u64, Listing> {
    let batches = vec![
        vec![file("s1.txt", 411)],
        vec![file("s2.txt", 412)],
        vec![file("s3.txt", 413)],
    ];
    let sibling = if gated {
        Listing::Gated(batches)
    } else {
        Listing::Batches(batches, None)
    };
    HashMap::from([
        (ROOT, Listing::Entries(vec![dir("s", 410), dir("c", 401)])),
        (410, sibling),
        (401, Listing::Entries(disconnecting())),
        (405, Listing::Entries(vec![file("never.txt", 406)])),
    ])
}

fn sequential_run(cancel: bool) -> Result<(Collected, Arc<Faults>), String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = sink_with_result(&ids)?;
    let (faults, root) = FaultDir::cancelling_on_disconnect(root_beside_a_disconnect(), 0);
    let checkpoint = || match cancel && faults.cancel.load(SeqCst) {
        true => Checkpoint::Cancel,
        false => Checkpoint::Continue,
    };
    let outcome = crate::scan::collect(&sink, ids.next_id(), root, &ids, checkpoint, |_| true);
    Ok((pinned(outcome, &sink)?, faults))
}

/// One parallel run; more than one worker gates the later reads of `s`.
fn parallel_run(
    workers: usize,
    seed: u64,
    cancel: bool,
) -> Result<(Collected, Arc<Faults>), String> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = sink_with_result(&ids)?;
    let listings = sibling_beside_a_disconnect(workers > 1);
    let (faults, root) = FaultDir::cancelling_on_disconnect(listings, seed);
    let context = Cancelling {
        flag: cancel.then(|| Arc::clone(&faults.cancel)),
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
    Ok((pinned(outcome, &sink)?, faults))
}

fn names(collected: &Collected) -> Vec<Vec<Vec<u8>>> {
    collected
        .run
        .summary
        .0
        .iter()
        .map(|item| item.1.clone())
        .collect()
}

#[test]
fn without_cancellation_a_started_folder_is_read_to_its_end() -> Result<(), String> {
    let _guard = worker_test_lock();
    let (baseline, faults) = sequential_run(false)?;
    // Three batches and the end of the listing.
    assert_eq!(faults.reads(ROOT), 4);
    assert!(names(&baseline).contains(&path(&["y.txt"])));
    for run in 0..16_u64 {
        let workers = WORKERS[run as usize % WORKERS.len()];
        let label = format!("run={run} workers={workers}");
        let (collected, faults) = parallel_run(workers, run + 1, false)?;
        assert_eq!(faults.gate_timeouts.load(SeqCst), 0, "{label}");
        // s either started before the volume closed and is read to its end,
        // or it is a gap that was never read.
        let reads = faults.reads(410);
        assert!(reads == 0 || reads == 4, "{label}: s read {reads} times");
        let end = names(&collected).contains(&path(&["s", "s3.txt"]));
        assert_eq!(end, reads == 4, "{label}");
        assert!(!faults.opened(405), "{label}");
    }
    Ok(())
}

#[test]
fn cancellation_after_a_disconnection_stops_reading_at_the_next_call() -> Result<(), String> {
    let _guard = worker_test_lock();
    let (baseline, faults) = sequential_run(true)?;
    assert_eq!(faults.calls_after_cancel.load(SeqCst), 0);
    // The root's remaining batches are never read.
    assert_eq!(faults.reads(ROOT), 1);
    assert_eq!(baseline.run.completeness, ResultCompleteness::Cancelled);
    assert_eq!(
        baseline.run.outcome.unprocessed_summary.as_deref(),
        Some("unprocessed_roots=1")
    );
    for run in 0..16_u64 {
        let workers = WORKERS[run as usize % WORKERS.len()];
        let label = format!("run={run} workers={workers}");
        let (collected, faults) = parallel_run(workers, run + 1, true)?;
        let late = faults.calls_after_cancel.load(SeqCst);
        // Only a worker already between its check and its call can start one.
        assert!(late < workers, "{label}: {late} calls after cancellation");
        if workers == 1 {
            assert_eq!(late, 0, "{label}");
        } else {
            // A read of s in flight when the job was cancelled returns, and
            // no later read of s is issued.
            assert_eq!(faults.gate_timeouts.load(SeqCst), 0, "{label}");
            assert!(
                faults.reads(410) <= 2,
                "{label}: s read {} times",
                faults.reads(410)
            );
            assert!(
                !names(&collected).contains(&path(&["s", "s3.txt"])),
                "{label}"
            );
        }
        assert!(!faults.opened(405), "{label}");
        assert_eq!(
            collected.run.completeness,
            ResultCompleteness::Cancelled,
            "{label}"
        );
        assert!(!collected.run.outcome.completed_scope, "{label}");
    }
    Ok(())
}
