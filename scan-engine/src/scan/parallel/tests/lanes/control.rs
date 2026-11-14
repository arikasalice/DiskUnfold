//! Pause, cancellation and disconnection with several lanes (FR11.HD1.app-host.DD1).
use super::{remote::*, *};

const SHAPE: Shape = Shape {
    depth: 2,
    dirs: 3,
    files: 4,
    batch: 2,
};

/// Lane 0 blocks its first read of "d0" on `gate`; lane 1 reads slowly.
fn gated_probe(gate: &Arc<Gate>) -> Arc<Probe> {
    let (gate, first) = (gate.clone(), AtomicBool::new(true));
    Probe::new(
        SHAPE,
        2,
        Box::new(move |root, path, call| {
            if call == Call::Read && root == 1 {
                thread::sleep(Duration::from_millis(2));
            }
            let target = root == 0 && call == Call::Read && path == [b"d0".to_vec()];
            if target && first.swap(false, Ordering::SeqCst) {
                gate.wait()?;
            }
            Ok(())
        }),
    )
}

/// Asserts that no open or read returns during a short wait.
fn assert_no_calls(probe: &Probe) {
    let calls = probe.calls();
    thread::sleep(Duration::from_millis(100));
    assert_eq!(probe.calls(), calls);
}

#[test]
fn pause_parks_every_lane_and_resume_reads_nothing_twice() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let (gate, remote) = (Arc::new(Gate::default()), Arc::new(Remote::default()));
    let (_open, _unpark) = (Opened(gate.clone()), Unpark(remote.clone()));
    let probe = gated_probe(&gate);
    let (job_probe, context) = (probe.clone(), RemoteContext(remote.clone()));
    let collector = thread::spawn(move || run_forest(&job_probe, vec![0, 1], 4, 2, &context));
    gate.until(|state| state.entered == 1)?;
    remote.set(|state| state.paused = true)?;
    // The call in flight is not interrupted; every other flow parks.
    remote.until(|state| state.active == 4 && state.parked == 3)?;
    assert_no_calls(&probe);
    gate.release();
    remote.until(|state| state.parked == 4)?;
    assert_no_calls(&probe);
    remote.set(|state| state.paused = false)?;
    let run = collector.join().map_err(|_| "collector panicked")??;
    assert_eq!(run.completeness, ResultCompleteness::Complete);
    assert_eq!(run.rows.len(), 2 * SHAPE.items());
    assert_eq!(sum(&probe.reads), 2 * SHAPE.reads());
    assert_eq!(sum(&probe.opens), 2 * (SHAPE.directories() - 1));
    Ok(())
}

#[test]
fn cancel_stops_every_lane_after_the_call_in_flight() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let (gate, remote) = (Arc::new(Gate::default()), Arc::new(Remote::default()));
    let (_open, _unpark) = (Opened(gate.clone()), Unpark(remote.clone()));
    let probe = gated_probe(&gate);
    let (job_probe, context) = (probe.clone(), RemoteContext(remote.clone()));
    let collector = thread::spawn(move || run_forest(&job_probe, vec![0, 1], 4, 2, &context));
    gate.until(|state| state.entered == 1)?;
    remote.set(|state| state.cancelled = true)?;
    // Only the worker inside the blocked call still holds its flow.
    remote.until(|state| state.active == 1)?;
    let calls = probe.calls();
    assert_no_calls(&probe);
    gate.release();
    let run = collector.join().map_err(|_| "collector panicked")??;
    assert!(!run.outcome.completed_scope);
    assert_eq!(run.completeness, ResultCompleteness::Cancelled);
    assert_eq!(probe.calls(), calls + 1);
    assert_eq!(probe.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn a_disconnected_root_closes_only_the_rest_of_its_own_lane() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let shape = Shape {
        depth: 2,
        dirs: 3,
        files: 2,
        batch: 4,
    };
    let root_reads = AtomicUsize::new(0);
    // The second read of root 0 fails as disconnected after its first batch
    // queued "d0" to "d2"; one worker per lane keeps that order fixed.
    let probe = Probe::new(
        shape,
        2,
        Box::new(move |root, path, call| {
            let second = root == 0
                && call == Call::Read
                && path.is_empty()
                && root_reads.fetch_add(1, Ordering::SeqCst) == 1;
            if second {
                return Err(io_error("scan.disconnected"));
            }
            Ok(())
        }),
    );
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let run = run_forest(&probe, vec![0, 1], 4, 1, &context)?;
    assert_eq!(run.completeness, ResultCompleteness::Missing);
    // Queued children of the closed root are closed without being opened.
    assert_eq!(probe.opens[0].load(Ordering::SeqCst), 0);
    let gaps: Vec<_> = run
        .issues
        .iter()
        .filter(|issue| issue.starts_with("ResultScopeGap"))
        .collect();
    assert_eq!(gaps.len(), 4, "{gaps:?}");
    assert!(
        gaps.iter()
            .all(|gap| gap.contains("root_index: 0") && gap.contains("scan.disconnected"))
    );
    // The other lane collects its whole root.
    let other = run.rows.iter().filter(|row| row.starts_with("1 ")).count();
    assert_eq!(other, shape.items());
    assert_eq!(probe.finished(1), shape.directories());
    assert!(
        !run.issues
            .iter()
            .any(|issue| issue.contains("root_index: 1"))
    );
    Ok(())
}
