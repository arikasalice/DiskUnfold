use super::*;

#[test]
fn static_strategy_uses_kind_start_quiet_fast_and_caps() -> Result<(), &'static str> {
    for (kind, auto, quiet, permit) in [
        (DeviceKind::Ssd, 8, 4, 64),
        (DeviceKind::Hdd, 2, 1, 4),
        (DeviceKind::Unknown, 4, 2, 16),
    ] {
        for (mode, expected) in [
            (ScanResourceMode::Auto, auto),
            (ScanResourceMode::Quiet, quiet),
            (ScanResourceMode::Fast, permit),
        ] {
            let policy = resolve(
                Condition {
                    mode,
                    user_limit: None,
                },
                kind,
                None,
            )
            .ok_or("valid policy was rejected")?;
            assert_eq!(policy.effective_limit, expected);
            assert_eq!(policy.permit, permit);
            assert_eq!(policy.share, None);
            assert_eq!(policy.strategy.id, "resource.static");
            assert_eq!(policy.strategy.version, 1);
        }
        let capped = resolve(
            Condition {
                mode: ScanResourceMode::Fast,
                user_limit: Some(3),
            },
            kind,
            NonZeroU32::new(2),
        )
        .ok_or("capped policy was rejected")?;
        assert_eq!(capped.effective_limit, permit.min(3).min(2));
    }
    Ok(())
}

#[test]
fn per_job_limits_use_the_maximum_lane_permit() {
    assert!(!valid_user_limit(Some(0), 64));
    assert!(valid_user_limit(Some(4), 4));
    assert!(!valid_user_limit(Some(5), 4));
    assert!(valid_user_limit(Some(64), 64));
    assert!(!valid_user_limit(Some(65), 64));
    assert!(valid_user_limit(Some(16), 16));
    assert!(!valid_user_limit(Some(17), 16));
    assert!(valid_user_limit(None, 16));
}

#[test]
fn invalid_user_limit_falls_back_to_one_worker() {
    let condition = Condition {
        mode: ScanResourceMode::Fast,
        user_limit: Some(0),
    };
    assert!(resolve(condition, DeviceKind::Unknown, None).is_none());
    assert_eq!(
        resolve_or_fallback(condition, DeviceKind::Unknown, None).effective_limit,
        1
    );
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
    assert_eq!(lane.share, None);
    Ok(())
}
