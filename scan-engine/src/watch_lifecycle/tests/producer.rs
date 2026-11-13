use super::super::unit::Unit;
use super::*;
use crate::watch_coalescer::{Coalescer, Context, Cursor, Gap};

#[test]
fn change_sequence_is_continuous_across_status_and_restart() -> Result<(), Error> {
    let mut r = registry();
    let id = [32; 16];
    let start = r.start(definition(32), &[9], capability(), 1)?;
    let t = token(&start.actions);
    let active = r.registration(
        id,
        t,
        1,
        &Registration::Acquired {
            handle: 32,
            uncovered: vec![],
        },
        2,
    )?;
    let Action::Changes(begin) = &active[0] else {
        panic!("begin")
    };
    assert_eq!((begin.sequence, begin.change_sequence), (2, 1));
    r.signal(id, t, 1, &signal(3))?;
    let flushed = r.tick(id, t, 1, 1003)?;
    let [Action::Changes(changed)] = &flushed[..] else {
        panic!("change")
    };
    assert_eq!((changed.sequence, changed.change_sequence), (4, 2));
    let stopped = r.stop(id, 1004)?;
    let Action::Changes(gap) = &stopped[0] else {
        panic!("stop gap")
    };
    assert_eq!((gap.sequence, gap.change_sequence), (5, 3));
    r.removed(id, t, 32, Completion::Removed, 1005)?;
    let restart = r.start(definition(32), &[9], capability(), 1006)?;
    let next = token(&restart.actions);
    let resumed = r.registration(
        id,
        next,
        2,
        &Registration::Acquired {
            handle: 33,
            uncovered: vec![],
        },
        1007,
    )?;
    let Action::Changes(begin) = &resumed[0] else {
        panic!("resume")
    };
    assert_eq!((begin.sequence, begin.change_sequence), (9, 4));
    let cut = r.incremental_snapshot(id, 1007)?;
    assert_eq!(
        (cut.sequence, cut.last_change_sequence, cut.observed_at),
        (10, 4, 1007)
    );
    assert_eq!(cut.generation, 2);
    assert_eq!(cut.definition.watch_ref, id);
    Ok(())
}
fn buffered_unit_with_queue(
    sequence: u64,
    change_sequence: u64,
    queue_limit: usize,
) -> Result<Unit, Error> {
    let cfg = Config {
        gap_limit: 8,
        queue_limit,
        ..Config::default()
    };
    let mut unit = Unit::new(definition(33), capability(), (9, 1), cfg);
    unit.state = State::Active;
    unit.generation = 1;
    unit.attempt_generation = 1;
    unit.sequence = sequence;
    unit.change_sequence = change_sequence;
    unit.last_gap = 6;
    unit.handle = Some(33);
    unit.last_time = 10;
    unit.last_observed = Some(10);
    unit.gaps = (1..=6)
        .map(|id| Gap {
            id,
            reason: GapReason::SignalLost,
            path: None,
            start: Some(id),
            end: Some(id + 1),
        })
        .collect();
    let ctx = Context {
        watch_ref: [33; 16],
        scope_revision: [8; 16],
        volume: vec![9],
        generation: 1,
    };
    let mut stream = Coalescer::new(
        ctx,
        cfg,
        Cursor {
            sequence,
            change_sequence,
            last_gap_id: 6,
        },
        vec![],
    )?;
    stream.signal(1, &signal(11))?;
    unit.last_time = 11;
    unit.last_observed = Some(11);
    unit.stream = Some(stream);
    Ok(unit)
}
#[test]
fn buffered_change_then_compaction_then_trigger_then_stop() -> Result<(), Error> {
    let mut unit = buffered_unit_with_queue(10, 5, 4096)?;
    let actions = unit.signal(9, 1, &signal(12))?;
    let [Action::Changes(prior), Action::Changes(compacted)] = &actions[..] else {
        panic!("ordered")
    };
    assert_eq!((prior.change_sequence, compacted.change_sequence), (6, 7));
    assert_eq!(prior.changes.len(), 1);
    assert_eq!(compacted.replaced_gap_ids, vec![1, 2, 3, 4]);
    assert!(compacted.changes.is_empty());
    let stop = unit.stop(13)?;
    let [
        Action::Changes(trigger),
        Action::Changes(gap),
        Action::Status(_),
        Action::Remove { .. },
    ] = &stop[..]
    else {
        panic!("stop")
    };
    assert_eq!((trigger.change_sequence, gap.change_sequence), (8, 9));
    assert_eq!(trigger.changes[0].first, 12);
    assert!(trigger.replaced_gap_ids.is_empty());
    Ok(())
}
#[test]
fn buffered_change_then_compaction_then_loss_notice() -> Result<(), Error> {
    let mut unit = buffered_unit_with_queue(10, 5, 4096)?;
    let actions = unit.signal_lost(9, 1, None, 12)?;
    let [
        Action::Changes(prior),
        Action::Changes(compacted),
        Action::Changes(loss),
    ] = &actions[..]
    else {
        panic!("ordered loss")
    };
    assert_eq!(
        (
            prior.change_sequence,
            compacted.change_sequence,
            loss.change_sequence
        ),
        (6, 7, 8)
    );
    assert_eq!(compacted.replaced_gap_ids, vec![1, 2, 3, 4]);
    assert!(
        loss.gaps
            .iter()
            .any(|gap| gap.reason == GapReason::SignalLost && gap.id == 8)
    );
    Ok(())
}
#[test]
fn failed_compaction_keeps_buffer_ledger_and_counters() -> Result<(), Error> {
    let mut unit = buffered_unit_with_queue(u64::MAX - 1, 5, 4096)?;
    let before = unit.gaps.clone();
    assert!(matches!(
        unit.signal(9, 1, &signal(12)),
        Err(Error::Coalescer(_))
    ));
    assert_eq!(unit.gaps, before);
    assert_eq!(
        (
            unit.sequence,
            unit.change_sequence,
            unit.last_gap,
            unit.last_time
        ),
        (u64::MAX - 1, 5, 6, 11)
    );
    assert_eq!(
        unit.stream
            .as_ref()
            .ok_or(Error::InvalidState)?
            .buffered_len(),
        1
    );
    Ok(())
}
#[test]
fn closed_partial_capability_summary_becomes_unavailable() -> Result<(), Error> {
    let cfg = Config {
        gap_limit: 4,
        ..Config::default()
    };
    let mut unit = Unit::new(definition(34), capability(), (1, 1), cfg);
    unit.generation = 1;
    unit.sequence = 4;
    unit.change_sequence = 2;
    unit.last_gap = 3;
    unit.last_time = 10;
    unit.gaps = vec![
        Gap {
            id: 1,
            reason: GapReason::PartialCapability,
            path: Some(vec![b"sub".to_vec()]),
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
            reason: GapReason::Stopped,
            path: None,
            start: Some(3),
            end: None,
        },
    ];
    let actions = unit.compact_for_capacity(10)?.ok_or(Error::NotFound)?;
    let [Action::Changes(batch)] = &actions[..] else {
        panic!("summary")
    };
    assert_eq!(batch.replaced_gap_ids, vec![1, 2]);
    assert!(
        batch
            .gaps
            .iter()
            .any(|g| g.id == 4 && g.reason == GapReason::Unavailable && g.path.is_none())
    );
    assert!(unit.gaps.iter().any(|g| g.id == 3 && g.end.is_none()));
    Ok(())
}

#[test]
fn trigger_failure_after_trial_compaction_rolls_back_everything() -> Result<(), Error> {
    let mut unit = buffered_unit_with_queue(u64::MAX - 2, 5, 2)?;
    let before = unit.gaps.clone();
    let trigger = Signal {
        path: Some(vec![b"new".to_vec(), b"dir".to_vec()]),
        directory_subtree: true,
        kind: ChangeKind::Create,
        observed_at: 12,
    };
    assert!(matches!(
        unit.signal(9, 1, &trigger),
        Err(Error::Coalescer(_))
    ));
    assert_eq!(unit.gaps, before);
    assert_eq!(
        (
            unit.sequence,
            unit.change_sequence,
            unit.last_gap,
            unit.last_time
        ),
        (u64::MAX - 2, 5, 6, 11)
    );
    assert_eq!(
        unit.stream
            .as_ref()
            .ok_or(Error::InvalidState)?
            .buffered_len(),
        1
    );
    Ok(())
}
