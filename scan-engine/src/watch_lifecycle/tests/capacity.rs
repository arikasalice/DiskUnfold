use super::*;

#[test]
fn full_gap_budget_fences_signal_and_marks_whole_unit_unavailable() -> Result<(), Error> {
    let cfg = Config {
        gap_limit: 3,
        ..Config::default()
    };
    let mut r = Registry::new(2, 4, cfg)?;
    let id = [21; 16];
    let start = r.start(definition(21), &[9], capability(), 1)?;
    let t = token(&start.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 21,
            uncovered: vec![],
        },
        2,
    )?;
    let actions = r.signal(id, t, 1, &signal(3))?;
    let [Action::Changes(batch), Action::Status(status)] = &actions[..] else {
        panic!("loss fence")
    };
    assert_eq!(status.state, State::Lost);
    assert!(batch.gaps.iter().any(|g| g.reason == GapReason::Unavailable
        && g.path.is_none()
        && g.start == Some(2)
        && g.end.is_none()));
    assert!(r.signal(id, t, 1, &signal(4))?.is_empty());
    assert!(
        r.stop(id, 5)?
            .iter()
            .any(|a| matches!(a, Action::Remove { handle: 21, .. }))
    );
    Ok(())
}

#[test]
fn loss_gap_preserves_slots_for_interruption_and_stop() -> Result<(), Error> {
    let cfg = Config {
        gap_limit: 4,
        ..Config::default()
    };
    let mut r = Registry::new(2, 4, cfg)?;
    let id = [27; 16];
    let start = r.start(definition(27), &[9], capability(), 1)?;
    let t = token(&start.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 27,
            uncovered: vec![],
        },
        2,
    )?;
    r.signal_lost(id, t, 1, None, 3)?;
    let actions = r.interrupted(id, t, 1, Interruption::Disconnected, 4)?;
    assert!(matches!(
        actions.last(),
        Some(Action::Status(Status {
            state: State::Lost,
            ..
        }))
    ));
    assert!(r.signal(id, t, 1, &signal(5))?.is_empty());
    let actions = r.stop(id, 6)?;
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Remove { handle: 27, .. }))
    );
    let (_, _, gaps) = r.snapshot(id)?;
    assert_eq!(gaps.len(), 4);
    assert!(gaps.iter().any(|g| g.reason == GapReason::SignalLost));
    assert!(
        gaps.iter()
            .any(|g| g.reason == GapReason::Unavailable && g.end.is_none())
    );
    assert!(
        gaps.iter()
            .any(|g| g.reason == GapReason::Stopped && g.end.is_none())
    );
    Ok(())
}

#[test]
fn next_signal_compacts_then_is_processed_without_gate() -> Result<(), Error> {
    let cfg = Config {
        gap_limit: 4,
        ..Config::default()
    };
    let mut r = Registry::new(2, 4, cfg)?;
    let id = [28; 16];
    let start = r.start(definition(28), &[9], capability(), 1)?;
    let t = token(&start.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 28,
            uncovered: vec![],
        },
        2,
    )?;
    r.signal_lost(id, t, 1, None, 3)?;
    let out = r.signal(id, t, 1, &signal(4))?;
    let [Action::Changes(batch)] = &out[..] else {
        panic!("compaction")
    };
    assert_eq!(batch.replaced_gap_ids, vec![1, 2]);
    let summary = &batch.gaps[0];
    assert_eq!(
        (summary.id, summary.reason, summary.start, summary.end),
        (3, GapReason::SignalLost, None, Some(3))
    );
    let snapshot = r.incremental_snapshot(id, 4)?;
    assert_eq!(snapshot.last_change_sequence, batch.change_sequence);
    assert_eq!(snapshot.gaps, vec![summary.clone()]);
    let next = r.tick(id, t, 1, 1004)?;
    let [Action::Changes(changes)] = &next[..] else {
        panic!("trigger signal")
    };
    assert_eq!(changes.change_sequence, batch.change_sequence + 1);
    assert_eq!(changes.changes.len(), 1);
    assert!(changes.replaced_gap_ids.is_empty());
    Ok(())
}
#[test]
fn compaction_covers_oldest_half_preserves_open_gap_and_restart() -> Result<(), Error> {
    let cfg = Config {
        gap_limit: 8,
        ..Config::default()
    };
    let mut r = Registry::new(2, 4, cfg)?;
    let id = [29; 16];
    let mut cap = capability();
    cap.notification = Support::Partial;
    let start = r.start(definition(29), &[9], cap, 1)?;
    let t = token(&start.actions);
    r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 29,
            uncovered: vec![vec![b"uncovered".to_vec()]],
        },
        2,
    )?;
    r.signal(id, t, 1, &signal(3))?;
    for at in 4..=7 {
        r.signal_lost(id, t, 1, None, at)?;
    }
    let (_, _, before) = r.snapshot(id)?;
    assert_eq!(before.len(), 6);
    let out = r.signal(id, t, 1, &signal(8))?;
    let [Action::Changes(batch)] = &out[..] else {
        panic!("compaction")
    };
    assert_eq!(batch.replaced_gap_ids, vec![1, 3, 4, 5]);
    let summary = batch
        .gaps
        .iter()
        .find(|g| g.id == 7)
        .ok_or(Error::NotFound)?;
    assert_eq!(summary.reason, GapReason::SignalLost);
    assert_eq!(
        (summary.start, summary.end, summary.path.as_ref()),
        (Some(1), Some(6), None)
    );
    assert!(batch.gaps.iter().any(|g| g.id == 2 && g.end.is_none()));
    let (_, _, after) = r.snapshot(id)?;
    assert_eq!(after.len(), 3);
    assert!(after.iter().any(|g| g == summary));
    assert!(after.iter().any(|g| g.id == 2 && g.end.is_none()));
    assert!(!after.iter().any(|g| batch.replaced_gap_ids.contains(&g.id)));
    let stopped = r.stop(id, 9)?;
    assert!(
        stopped
            .iter()
            .any(|a| matches!(a, Action::Remove { handle: 29, .. }))
    );
    r.removed(id, t, 29, Completion::Removed, 10)?;
    let restart = r.start(definition(29), &[9], cap, 11)?;
    let next = token(&restart.actions);
    let resumed = r.registration(
        id,
        next,
        2,
        &Registration::Acquired {
            handle: 30,
            uncovered: vec![vec![b"uncovered".to_vec()]],
        },
        12,
    )?;
    let first = resumed
        .iter()
        .find_map(|action| {
            if let Action::Changes(batch) = action {
                Some(batch)
            } else {
                None
            }
        })
        .ok_or(Error::NotFound)?;
    assert_eq!(first.context.generation, 2);
    assert!(first.change_sequence > batch.change_sequence);
    let (_, _, retained) = r.snapshot(id)?;
    assert!(retained.iter().any(|g| g.id == 7));
    Ok(())
}
#[test]
fn consumer_resync_gap_is_never_producer_compacted() {
    use super::super::unit::Unit;
    use crate::watch_coalescer::Gap;
    let cfg = Config {
        gap_limit: 4,
        ..Config::default()
    };
    let mut u = Unit::new(definition(30), capability(), (1, 1), cfg);
    u.generation = 1;
    u.sequence = 4;
    u.last_gap = 3;
    u.last_time = 10;
    u.gaps = vec![
        Gap {
            id: 1,
            reason: GapReason::ConsumerResync,
            path: Some(vec![b"a".to_vec()]),
            start: Some(1),
            end: Some(2),
        },
        Gap {
            id: 2,
            reason: GapReason::SignalLost,
            path: None,
            start: Some(2),
            end: Some(3),
        },
        Gap {
            id: 3,
            reason: GapReason::Unavailable,
            path: None,
            start: Some(3),
            end: None,
        },
    ];
    let before = u.gaps.clone();
    assert_eq!(u.compact_for_capacity(10), Ok(None));
    assert_eq!(u.gaps, before);
    assert_eq!((u.sequence, u.last_gap), (4, 3));
}
#[test]
fn unknown_start_and_strongest_reason_widen_summary_conservatively() -> Result<(), Error> {
    use super::super::unit::Unit;
    use crate::watch_coalescer::Gap;
    let cfg = Config {
        gap_limit: 4,
        ..Config::default()
    };
    let mut u = Unit::new(definition(31), capability(), (1, 1), cfg);
    u.generation = 1;
    u.sequence = 4;
    u.last_gap = 3;
    u.last_time = 10;
    u.gaps = vec![
        Gap {
            id: 1,
            reason: GapReason::BeforeStart,
            path: Some(vec![b"a".to_vec()]),
            start: None,
            end: Some(2),
        },
        Gap {
            id: 2,
            reason: GapReason::Unavailable,
            path: Some(vec![b"b".to_vec()]),
            start: Some(2),
            end: Some(8),
        },
        Gap {
            id: 3,
            reason: GapReason::PartialCapability,
            path: None,
            start: Some(8),
            end: None,
        },
    ];
    let actions = u.compact_for_capacity(10)?.ok_or(Error::NotFound)?;
    let [Action::Changes(batch)] = &actions[..] else {
        panic!("summary")
    };
    let summary = batch
        .gaps
        .iter()
        .find(|g| g.id == 4)
        .ok_or(Error::NotFound)?;
    assert_eq!(
        (
            summary.reason,
            summary.start,
            summary.end,
            summary.path.as_ref()
        ),
        (GapReason::Unavailable, None, Some(8), None)
    );
    assert!(u.gaps.iter().any(|g| g.id == 3 && g.end.is_none()));
    assert!(u.gaps.iter().any(|g| g == summary));
    Ok(())
}
