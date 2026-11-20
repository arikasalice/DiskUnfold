use super::*;

#[test]
fn static_strategy_resolves_modes_user_caps_device_permits_and_minimum_one()
-> Result<(), &'static str> {
    for (mode, user_limit, permit, expected) in [
        (ScanResourceMode::Auto, None, 16, 4),
        (ScanResourceMode::Quiet, None, 16, 2),
        (ScanResourceMode::Fast, None, 16, 16),
        (ScanResourceMode::Auto, Some(2), 16, 2),
        (ScanResourceMode::Fast, Some(3), 16, 3),
        (ScanResourceMode::Quiet, Some(1), 16, 1),
        (ScanResourceMode::Auto, None, 3, 3),
        (ScanResourceMode::Fast, None, 1, 1),
    ] {
        let Some(policy) = resolve(Condition { mode, user_limit }, permit) else {
            return Err("valid policy was rejected");
        };
        assert_eq!(policy.effective_limit, expected);
        assert_eq!(policy.permit, permit);
        assert_eq!(policy.strategy.id, "resource.static");
        assert_eq!(policy.strategy.version, 1);
    }
    Ok(())
}

#[test]
fn invalid_permits_and_limits_are_rejected_and_fallback_is_one() {
    let condition = Condition {
        mode: ScanResourceMode::Fast,
        user_limit: Some(2),
    };
    assert!(resolve(condition, 0).is_none());
    assert!(resolve(condition, 1).is_none());
    assert!(!valid_user_limit(Some(0)));
    assert!(!valid_user_limit(Some(17)));
    assert!(valid_user_limit(Some(16)));
    assert!(valid_user_limit(None));
    assert_eq!(resolve_or_fallback(condition, 1).effective_limit, 1);
}

#[test]
fn a_queued_update_is_accepted_then_used_when_the_lane_attaches() -> Result<(), &'static str> {
    use std::sync::{Arc, Condvar, Mutex};

    let session = Arc::new(crate::scan::parallel::ScanControlSession::new(1));
    let handle = session.handle();
    let (outcome, lanes) = handle
        .set_resource(ScanResourceMode::Quiet, Some(1))
        .map_err(|_| "queued update failed")?;
    assert_eq!(
        outcome,
        du_core_contracts::analysis_ipc::ScanSetResourceOutcome::Accepted
    );
    assert!(lanes.is_empty());

    let queue = Arc::new(Mutex::new(crate::scan::parallel::Queue::for_test(
        vec![0],
        4,
    )));
    let wake = Arc::new(Condvar::new());
    session.attach(&queue, &wake);
    let queue = queue.lock().map_err(|_| "queue lock poisoned")?;
    let lane = queue
        .lanes
        .resource_rows()
        .into_iter()
        .next()
        .ok_or("resource lane was not attached")?;
    assert_eq!(lane.effective_limit, 1);
    assert_eq!(lane.permit, Some(16));
    Ok(())
}
