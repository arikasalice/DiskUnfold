use super::*;

#[test]
fn active_limit_returns_unsupported_and_preserves_other_unit() -> Result<(), Error> {
    let mut r = Registry::new(1, 4, Config::default())?;
    let first = r.start(definition(10), &[9], capability(), 1)?;
    let token = token(&first.actions);
    let second = r.start(definition(11), &[9], capability(), 2)?;
    assert!(matches!(
        &second.actions[..],
        [Action::Status(Status {
            state: State::Unsupported,
            reason: Some("watch.resource_limit"),
            ..
        })]
    ));
    assert_eq!(r.snapshot([10; 16])?.0, State::Starting);
    assert_eq!(r.snapshot([11; 16])?.0, State::Unsupported);
    assert!(
        r.registration(
            [10; 16],
            token,
            1,
            &Registration::Acquired {
                handle: 10,
                uncovered: vec![]
            },
            3
        )
        .is_ok()
    );
    Ok(())
}
#[test]
fn interrupted_watch_fences_signals_and_keeps_unavailable_gap() -> Result<(), Error> {
    let mut r = registry();
    let id = [12; 16];
    let start = r.start(definition(12), &[9], capability(), 1)?;
    let t = token(&start.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 12,
            uncovered: vec![],
        },
        2,
    )?;
    r.signal(id, t, 1, &signal(3))?;
    let out = r.interrupted(id, t, 1, Interruption::AccessLost, 4)?;
    assert!(matches!(
        &out[..],
        [
            Action::Changes(_),
            Action::Changes(_),
            Action::Status(Status {
                state: State::Lost,
                reason: Some("watch.access_lost"),
                ..
            })
        ]
    ));
    assert!(r.signal(id, t, 1, &signal(5))?.is_empty());
    assert!(
        r.interrupted(id, t, 1, Interruption::Disconnected, 5)?
            .is_empty()
    );
    let (_, _, gaps) = r.snapshot(id)?;
    assert!(
        gaps.iter()
            .any(|g| g.reason == GapReason::Unavailable && g.start == Some(3) && g.end.is_none())
    );
    let out = r.stop(id, 6)?;
    assert!(matches!(
        &out[..],
        [
            Action::Changes(_),
            Action::Status(Status {
                state: State::Stopping,
                ..
            }),
            Action::Remove { handle: 12, .. }
        ]
    ));
    r.removed(id, t, 12, Completion::Removed, 7)?;
    Ok(())
}
#[test]
fn loss_notice_and_timer_keep_generation_and_gap_evidence() -> Result<(), Error> {
    let mut r = registry();
    let id = [13; 16];
    let start = r.start(definition(13), &[9], capability(), 1)?;
    let t = token(&start.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 13,
            uncovered: vec![],
        },
        2,
    )?;
    r.signal(id, t, 1, &signal(3))?;
    let out = r.signal_lost(id, t, 1, None, 4)?;
    let [Action::Changes(batch)] = &out[..] else {
        panic!("loss batch")
    };
    assert!(
        batch
            .gaps
            .iter()
            .any(|g| g.reason == GapReason::SignalLost && g.start == Some(3) && g.end == Some(4))
    );
    assert_eq!(r.snapshot(id)?.0, State::Active);
    r.signal(id, t, 1, &signal(5))?;
    assert!(r.tick(id, t, 1, 1004)?.is_empty());
    assert_eq!(r.tick(id, t, 1, 1005)?.len(), 1);
    Ok(())
}
#[test]
fn oversized_partial_registration_can_be_cancelled_and_released() -> Result<(), Error> {
    let cfg = Config {
        gap_limit: 2,
        ..Config::default()
    };
    let mut r = Registry::new(2, 4, cfg)?;
    let id = [14; 16];
    let start = r.start(definition(14), &[9], capability(), 1)?;
    let t = token(&start.actions);
    let response = Registration::Acquired {
        handle: 14,
        uncovered: vec![vec![b"x".to_vec()], vec![b"y".to_vec()]],
    };
    assert_eq!(
        r.registration(id, t, 1, &response, 2),
        Err(Error::GapCapacity)
    );
    assert_eq!(r.snapshot(id)?.0, State::Starting);
    r.stop(id, 3)?;
    assert!(matches!(
        &r.registration(id, t, 1, &response, 4)?[..],
        [Action::Remove { handle: 14, .. }]
    ));
    Ok(())
}
#[test]
fn failed_remove_never_reports_stopped_or_issues_a_second_remove() -> Result<(), Error> {
    let mut r = registry();
    let id = [15; 16];
    let start = r.start(definition(15), &[9], capability(), 1)?;
    let t = token(&start.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 15,
            uncovered: vec![],
        },
        2,
    )?;
    r.stop(id, 3)?;
    assert_eq!(r.cleanup_ticket(id)?, None);
    assert!(r.removed(id, t, 15, Completion::Failed, 4)?.is_empty());
    assert_eq!(r.snapshot(id)?.0, State::Stopping);
    assert!(r.stop(id, 5)?.is_empty());
    let ticket = r.cleanup_ticket(id)?.ok_or(Error::NotFound)?;
    assert_eq!((ticket.token, ticket.generation, ticket.handle), (t, 1, 15));
    // The adapter owns the handle, performs explicit cleanup, then confirms it.
    assert!(matches!(
        &r.removed(id, ticket.token, ticket.handle, Completion::Removed, 6)?[..],
        [Action::Status(Status {
            state: State::Stopped,
            ..
        })]
    ));
    assert_eq!(r.cleanup_ticket(id)?, None);
    Ok(())
}
#[test]
fn completion_is_bound_to_both_attempt_and_generation() -> Result<(), Error> {
    let mut r = registry();
    let id = [16; 16];
    let start = r.start(definition(16), &[9], capability(), 1)?;
    let t = token(&start.actions);
    let response = Registration::Acquired {
        handle: 16,
        uncovered: vec![],
    };
    assert!(r.registration(id, t, 2, &response, 2)?.is_empty());
    assert_eq!(r.snapshot(id)?.0, State::Starting);
    r.registration(id, t, 1, &response, 2)?;
    r.stop(id, 3)?;
    r.removed(id, t, 16, Completion::Removed, 4)?;
    let start = r.start(definition(16), &[9], capability(), 5)?;
    let next = token(&start.actions);
    assert!(matches!(
        &start.actions[1],
        Action::Register { generation: 2, .. }
    ));
    assert!(r.registration(id, next, 1, &response, 6)?.is_empty());
    assert!(r.signal(id, t, 1, &signal(6))?.is_empty());
    let active = r.registration(
        id,
        next,
        2,
        &Registration::Acquired {
            handle: 17,
            uncovered: vec![],
        },
        7,
    )?;
    assert!(matches!(
        &active[1],
        Action::Status(Status {
            generation: 2,
            state: State::Active,
            ..
        })
    ));
    assert!(r.signal(id, next, 1, &signal(8))?.is_empty());
    Ok(())
}
#[test]
fn restart_and_changed_volume_preserve_old_open_gap() -> Result<(), Error> {
    let mut r = registry();
    let id = [17; 16];
    let start = r.start(definition(17), &[9], capability(), 1)?;
    let t = token(&start.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 17,
            uncovered: vec![],
        },
        2,
    )?;
    r.stop(id, 3)?;
    r.removed(id, t, 17, Completion::Removed, 4)?;
    let mut changed = definition(17);
    changed.volume = vec![10];
    assert_eq!(
        r.start(changed, &[10], capability(), 5),
        Err(Error::ScopeChanged)
    );
    let (_, _, gaps) = r.snapshot(id)?;
    assert!(
        gaps.iter()
            .any(|g| g.reason == GapReason::Stopped && g.end.is_none())
    );
    Ok(())
}
#[test]
fn duplicate_start_reuses_reference_and_retained_bound_is_explicit() -> Result<(), Error> {
    let mut r = Registry::new(1, 2, Config::default())?;
    let first = r.start(definition(18), &[9], capability(), 1)?;
    let again = r.start(definition(18), &[9], capability(), 2)?;
    assert!(again.already_watching);
    assert_eq!(again.watch_ref, first.watch_ref);
    assert!(again.actions.is_empty());
    r.start(definition(19), &[9], capability(), 2)?;
    assert_eq!(
        r.start(definition(20), &[9], capability(), 2),
        Err(Error::Capacity)
    );
    Ok(())
}
#[test]
fn changed_target_definition_is_rejected_without_touching_existing_unit() -> Result<(), Error> {
    let mut r = registry();
    let id = [22; 16];
    let start = r.start(definition(22), &[9], capability(), 1)?;
    let t = token(&start.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 22,
            uncovered: vec![],
        },
        2,
    )?;
    let mut changed = definition(22);
    changed.nested_mounts.push(vec![b"nested".to_vec()]);
    assert_eq!(
        r.start(changed, &[9], capability(), 3),
        Err(Error::ScopeChanged)
    );
    assert_eq!(r.snapshot(id)?.0, State::Active);
    Ok(())
}
#[test]
fn failed_registration_stop_and_retry_keep_gap_open_until_started() -> Result<(), Error> {
    let mut r = registry();
    let id = [23; 16];
    let start = r.start(definition(23), &[9], capability(), 1)?;
    let t = token(&start.actions);
    let lost = r.registration(id, t, 1, &Registration::Disconnected, 2)?;
    assert!(matches!(
        &lost[..],
        [Action::Status(Status {
            state: State::Lost,
            ..
        })]
    ));
    assert!(r.signal(id, t, 1, &signal(3))?.is_empty());
    let stop = r.stop(id, 4)?;
    assert!(matches!(
        &stop[..],
        [
            Action::Status(Status {
                state: State::Stopping,
                ..
            }),
            Action::Status(Status {
                state: State::Stopped,
                ..
            })
        ]
    ));
    let restart = r.start(definition(23), &[9], capability(), 5)?;
    let next = token(&restart.actions);
    let out = r.registration(
        id,
        next,
        1,
        &Registration::Acquired {
            handle: 23,
            uncovered: vec![],
        },
        6,
    )?;
    let Action::Changes(batch) = &out[0] else {
        panic!("closed gaps")
    };
    assert!(
        batch
            .gaps
            .iter()
            .any(|g| g.reason == GapReason::Unavailable && g.end == Some(6))
    );
    assert!(
        batch
            .gaps
            .iter()
            .any(|g| g.reason == GapReason::Stopped && g.end == Some(6))
    );
    Ok(())
}
#[test]
fn stop_during_failed_registration_finishes_without_remove() -> Result<(), Error> {
    let mut r = registry();
    let id = [24; 16];
    let started = r.start(definition(24), &[9], capability(), 1)?;
    let t = token(&started.actions);
    r.stop(id, 2)?;
    let out = r.registration(id, t, 1, &Registration::AccessLost, 3)?;
    assert!(matches!(
        &out[..],
        [Action::Status(Status {
            state: State::Stopped,
            ..
        })]
    ));
    assert!(
        r.registration(id, t, 1, &Registration::AccessLost, 4)?
            .is_empty()
    );
    assert_eq!(r.snapshot(id)?.0, State::Stopped);
    Ok(())
}
#[test]
fn unsupported_path_capability_marks_whole_root_unknown() -> Result<(), Error> {
    let mut r = registry();
    let id = [25; 16];
    let mut cap = capability();
    cap.path = Support::Unsupported;
    let started = r.start(definition(25), &[9], cap, 1)?;
    let t = token(&started.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 25,
            uncovered: vec![],
        },
        2,
    )?;
    r.signal(id, t, 1, &signal(3))?;
    let out = r.tick(id, t, 1, 1003)?;
    let [Action::Changes(batch)] = &out[..] else {
        panic!("batch")
    };
    assert_eq!(batch.changes.len(), 1);
    assert!(batch.changes[0].path.is_empty());
    assert!(batch.changes[0].subtree);
    assert!(batch.changes[0].kinds.contains(&ChangeKind::Unknown));
    Ok(())
}
#[test]
fn unsupported_path_capability_makes_loss_whole_root() -> Result<(), Error> {
    let mut r = registry();
    let id = [26; 16];
    let mut cap = capability();
    cap.path = Support::Unsupported;
    let started = r.start(definition(26), &[9], cap, 1)?;
    let t = token(&started.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 26,
            uncovered: vec![],
        },
        2,
    )?;
    let path = vec![b"untrusted".to_vec()];
    let out = r.signal_lost(id, t, 1, Some(&path), 3)?;
    let [Action::Changes(batch)] = &out[..] else {
        panic!("gap")
    };
    assert!(
        batch
            .gaps
            .iter()
            .any(|gap| gap.reason == GapReason::SignalLost && gap.path.is_none())
    );
    Ok(())
}
