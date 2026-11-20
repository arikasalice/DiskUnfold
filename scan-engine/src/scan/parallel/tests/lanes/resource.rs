//! Live lane resource changes are enforced at worker assignment boundaries.
use super::*;
use du_core_contracts::analysis_ipc::{ScanResourceMode, ScanSetResourceOutcome};

#[test]
fn lane_limit_bounds_fake_os_calls_and_live_changes_take_effect_next_assignment()
-> Result<(), TestError> {
    let _serial = worker_test_lock();
    let (first_gate, next_gate) = (Arc::new(Gate::default()), Arc::new(Gate::default()));
    let (_first_open, _next_open) = (Opened(first_gate.clone()), Opened(next_gate.clone()));
    let (first_hook, next_hook, first_count) =
        (first_gate.clone(), next_gate.clone(), AtomicUsize::new(0));
    let probe = Probe::new(
        Shape {
            depth: 1,
            dirs: 1,
            files: 0,
            batch: 1,
        },
        8,
        Box::new(move |_, _, _| {
            if first_count.fetch_add(1, Ordering::SeqCst) < 8 {
                first_hook.wait()?;
            } else {
                next_hook.wait()?;
            }
            Ok(())
        }),
    );
    let shared = shared_for(&probe, vec![0; 8], 4, QUEUED_DIRECTORIES)?;
    let session = Arc::new(ScanControlSession::with_resource_condition(
        8,
        ScanResourceMode::Fast,
        None,
    ));
    session.attach(&shared.queue, &shared.wake);
    let control = session.handle();

    let lane = shared
        .lane_report()
        .and_then(|report| report.lanes.into_iter().next())
        .ok_or("resource lane status missing")?;
    assert_eq!(lane.mode, Some(ScanResourceMode::Fast));
    assert_eq!(lane.permit, Some(16));
    let strategy = lane.strategy.ok_or("resource strategy missing")?;
    assert_eq!(strategy.id, "resource.static");
    assert_eq!(strategy.version, 1);

    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    run_workers_in(&shared, 8, 64, &context, |receiver, sender| {
        drop(sender);
        let observed = (|| -> Result<(), TestError> {
            first_gate.until(|state| state.waiting == 8)?;
            assert_eq!(probe.in_flight.load(Ordering::SeqCst), 8);

            let (outcome, lanes) = control
                .set_resource(ScanResourceMode::Quiet, Some(2))
                .map_err(|error| format!("lower resource limit: {error:?}"))?;
            assert_eq!(outcome, ScanSetResourceOutcome::Accepted);
            assert_eq!(lanes.first().map(|lane| lane.effective_limit), Some(2));
            assert_eq!(lanes.first().and_then(|lane| lane.permit), Some(16));
            assert_eq!(probe.in_flight.load(Ordering::SeqCst), 8);
            assert_eq!(
                first_gate.most(),
                8,
                "in-flight calls must not be interrupted"
            );

            first_gate.release();
            next_gate.until(|state| state.waiting == 2)?;
            assert_eq!(
                next_gate.most(),
                2,
                "lowered lane admitted more than two calls"
            );

            let (outcome, lanes) = control
                .set_resource(ScanResourceMode::Fast, Some(4))
                .map_err(|error| format!("raise resource limit: {error:?}"))?;
            assert_eq!(outcome, ScanSetResourceOutcome::Accepted);
            assert_eq!(lanes.first().map(|lane| lane.effective_limit), Some(4));
            next_gate.until(|state| state.waiting == 4)?;
            assert_eq!(next_gate.most(), 4, "raised lane did not admit four calls");
            Ok(())
        })();
        first_gate.release();
        next_gate.release();
        receiver.iter().for_each(drop);
        observed
    })??;
    assert_eq!(probe.in_flight.load(Ordering::SeqCst), 0);
    Ok(())
}
