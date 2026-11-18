use super::*;

#[test]
fn cancel_drains_two_lanes_under_backpressure_once_and_stops_reads() -> Result<(), String> {
    let _guard = worker_test_lock();
    assert_eq!(
        LANE_COUNT * (FIRST_BATCH_ITEMS + 1),
        BUFFERED_OBSERVATIONS,
        "two first batches and root reports must leave exactly one slot after one receive"
    );
    // The two fake adapters never call the OS. Their read barrier proves two
    // actual worker threads were inside adapter calls at the same time.
    let gate = Arc::new(Gate::default());
    let _cleanup = Cleanup(gate.clone());
    let context = Arc::new(MultiContext {
        gate: gate.clone(),
        flow_id: AtomicUsize::new(0),
    });
    let ids = Arc::new(Ids(AtomicUsize::new(1)));
    let store = IngestSink::default();
    let build = store
        .open_forest(
            Ids(AtomicUsize::new(99)).next_id(),
            vec![observed_root("C:\\A", 1), observed_root("D:\\B", 2)],
        )
        .map_err(|error| format!("{error:?}"))?;
    let (done_tx, done_rx) = mpsc::channel();
    let run_gate = gate.clone();
    let run_context = context.clone();
    let run_ids = ids.clone();
    let collector = std::thread::spawn(move || {
        let include = |_: &[Vec<u8>]| true;
        let roots: Vec<Box<dyn AnchoredDirectory>> = (0..LANE_COUNT)
            .map(|root| {
                Box::new(MultiBatch {
                    root,
                    gate: run_gate.clone(),
                }) as Box<dyn AnchoredDirectory>
            })
            .collect();
        let control = crate::scan::parallel::Control {
            ids: run_ids.as_ref(),
            context: run_context.as_ref(),
            limit: LANE_COUNT,
            policy: Policy::Legacy(&include),
        };
        let clock = ContextClock(run_context.as_ref());
        let result = body::run(
            build,
            roots,
            control,
            &mut JobCounts::default(),
            &clock,
            crate::scan::parallel::ScanOptions {
                devices: DevicePlan {
                    groups: vec![0, 1],
                    limit: 1,
                },
                diagnostics: None,
                scan_control: None,
                correlated_budget: None,
            },
        )
        .map_err(|error| format!("{error:?}"));
        let _ = done_tx.send(result);
    });

    let coordinated = coordinate_two_lane_cancel(&gate);

    // On every assertion, timeout, or early return, unblock both fake reads
    // and the publisher before joining the collector.
    if coordinated.is_ok() {
        gate.release_progress();
    } else {
        gate.cleanup();
    }
    let result = match done_rx.recv_timeout(TEST_TIMEOUT) {
        Ok(result) => result,
        Err(_) => {
            gate.cleanup();
            match done_rx.recv_timeout(TEST_TIMEOUT) {
                Ok(result) => result,
                Err(error) => {
                    drop(collector);
                    return Err(format!("collector remained blocked after cleanup: {error}"));
                }
            }
        }
    };
    collector.join().map_err(|_| "collector panicked")?;
    let (distinct_workers, pressure_held_until_both_boundaries) = coordinated?;
    assert!(
        distinct_workers,
        "both roots must be read by distinct workers"
    );
    assert!(pressure_held_until_both_boundaries);
    let (sealed_id, cancelled, _) = result?;
    assert!(cancelled);
    assert_eq!(context.flow_id.load(Ordering::SeqCst), 3); // prep plus two workers
    let state = gate.state.lock().map_err(|_| "gate lock")?;
    assert_eq!(state.calls, [2, 2]);
    assert_eq!(state.calls_after_cancel, [0, 0]);
    assert_eq!(state.cancel_boundaries, [true, true]);
    assert!(!state.progress_wait_timed_out);
    assert!(state.progress_released_after_boundaries);
    assert_eq!(state.worker_finishes, LANE_COUNT);
    drop(state);
    let version = store
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing cancelled version")?;
    assert_eq!(version.version_id(), &sealed_id);
    assert_eq!(version.completeness(), ResultCompleteness::Cancelled);
    assert_cancelled_batch_items(version.items());
    Ok(())
}
