use super::*;

#[test]
fn active_stop_flushes_and_fences_late_signals() -> Result<(), Error> {
    let mut r = registry();
    let id = [1; 16];
    let start = r.start(definition(1), &[9], capability(), 2)?;
    let t = token(&start.actions);
    let started = r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 42,
            uncovered: vec![],
        },
        3,
    )?;
    assert!(matches!(
        &started[..],
        [
            Action::Changes(_),
            Action::Status(Status {
                state: State::Active,
                ..
            })
        ]
    ));
    let Action::Changes(batch) = &started[0] else {
        panic!("start gap")
    };
    assert_eq!(batch.gaps[0].reason, GapReason::BeforeStart);
    assert_eq!((batch.sequence, batch.context.generation), (2, 1));
    assert!(r.signal(id, t, 1, &signal(4))?.is_empty());
    let actions = r.stop(id, 5)?;
    assert!(matches!(
        &actions[..],
        [
            Action::Changes(_),
            Action::Changes(_),
            Action::Status(Status {
                state: State::Stopping,
                ..
            }),
            Action::Remove { .. }
        ]
    ));
    let mut fake = FakePort::default();
    fake.apply(&actions);
    assert_eq!(fake.removed, vec![(t, 42)]);
    assert!(r.signal(id, t, 1, &signal(6))?.is_empty());
    assert!(r.stop(id, 7)?.is_empty());
    let closed = r.removed(id, t, 42, Completion::Removed, 8)?;
    assert!(matches!(
        &closed[..],
        [Action::Status(Status {
            state: State::Stopped,
            ..
        })]
    ));
    assert!(r.removed(id, t, 42, Completion::Removed, 9)?.is_empty());
    let (state, generation, gaps) = r.snapshot(id)?;
    assert_eq!((state, generation), (State::Stopped, 1));
    assert!(
        gaps.iter()
            .any(|g| g.reason == GapReason::Stopped && g.start == Some(5) && g.end.is_none())
    );
    Ok(())
}
#[test]
fn stop_while_registering_releases_acquired_handle_once() -> Result<(), Error> {
    let mut r = registry();
    let id = [2; 16];
    let start = r.start(definition(2), &[9], capability(), 2)?;
    let t = token(&start.actions);
    let stop = r.stop(id, 3)?;
    assert!(matches!(
        &stop[..],
        [Action::Status(Status {
            state: State::Stopping,
            ..
        })]
    ));
    assert!(r.signal(id, t, 1, &signal(4))?.is_empty());
    let response = Registration::Acquired {
        handle: 77,
        uncovered: vec![],
    };
    let actions = r.registration(id, t, 1, &response, 5)?;
    let mut fake = FakePort::default();
    fake.apply(&actions);
    assert_eq!(fake.removed, vec![(t, 77)]);
    assert!(r.registration(id, t, 1, &response, 6)?.is_empty());
    assert!(r.stop(id, 6)?.is_empty());
    r.removed(id, t, 77, Completion::Removed, 7)?;
    let (_, generation, gaps) = r.snapshot(id)?;
    assert_eq!(generation, 0);
    assert!(
        gaps.iter()
            .any(|g| g.reason == GapReason::BeforeStart && g.end == Some(3))
    );
    assert!(
        gaps.iter()
            .any(|g| g.reason == GapReason::Stopped && g.start == Some(3) && g.end.is_none())
    );
    let restart = r.start(definition(2), &[9], capability(), 8)?;
    let next = token(&restart.actions);
    assert_ne!(next, t);
    assert!(r.registration(id, t, 1, &response, 9)?.is_empty());
    assert!(r.signal(id, t, 1, &signal(9))?.is_empty());
    let active = r.registration(
        id,
        next,
        1,
        &Registration::Acquired {
            handle: 88,
            uncovered: vec![],
        },
        10,
    )?;
    let Action::Changes(batch) = &active[0] else {
        panic!("gaps")
    };
    assert!(
        batch
            .gaps
            .iter()
            .any(|g| g.reason == GapReason::BeforeStart)
    );
    assert!(
        batch
            .gaps
            .iter()
            .any(|g| g.reason == GapReason::Stopped && g.end == Some(10))
    );
    Ok(())
}
#[test]
fn unsupported_and_resource_limit_do_not_register() -> Result<(), Error> {
    let mut r = registry();
    let mut cap = capability();
    cap.notification = Support::Unsupported;
    let x = r.start(definition(3), &[9], cap, 1)?;
    assert!(matches!(
        &x.actions[..],
        [Action::Status(Status {
            state: State::Unsupported,
            ..
        })]
    ));
    let x = r.start(definition(4), &[9], capability(), 2)?;
    let t = token(&x.actions);
    let result = r.registration([4; 16], t, 1, &Registration::ResourceLimit, 3)?;
    assert!(matches!(
        &result[..],
        [Action::Status(Status {
            state: State::Unsupported,
            reason: Some("watch.resource_limit"),
            ..
        })]
    ));
    Ok(())
}
#[test]
fn partial_support_exposes_open_gaps_and_closes_them_on_stop() -> Result<(), Error> {
    let mut r = registry();
    let id = [5; 16];
    let mut cap = capability();
    cap.loss = Support::Unsupported;
    let start = r.start(definition(5), &[9], cap, 1)?;
    let t = token(&start.actions);
    let active = r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 1,
            uncovered: vec![],
        },
        2,
    )?;
    assert!(matches!(
        &active[1],
        Action::Status(Status {
            state: State::Degraded,
            ..
        })
    ));
    let (_, _, gaps) = r.snapshot(id)?;
    assert!(
        gaps.iter().any(|g| g.reason == GapReason::PartialCapability
            && g.path.is_none()
            && g.end.is_none())
    );
    r.stop(id, 3)?;
    let (_, _, gaps) = r.snapshot(id)?;
    assert!(
        gaps.iter()
            .any(|g| g.reason == GapReason::PartialCapability && g.end == Some(3))
    );
    Ok(())
}
