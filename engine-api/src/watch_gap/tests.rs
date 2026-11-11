#![allow(clippy::unwrap_used)] // Test fixtures fail immediately when setup is invalid.
use super::*;
use du_scan_engine::watch_coalescer::{Change, ChangeKind, Context, GapReason, Trust};
use std::collections::BTreeSet;

mod initial_snapshot;

fn definition(id: u8) -> Definition {
    Definition {
        watch_ref: [id; 16],
        scope_revision: [1; 16],
        target: vec![],
        entry_path: vec![1],
        volume_path: vec![],
        nested_mounts: vec![],
        volume: vec![2],
        baseline_end: Some(0),
    }
}
fn batch(id: u8, n: u64) -> Batch {
    Batch {
        context: Context {
            watch_ref: [id; 16],
            scope_revision: [1; 16],
            volume: vec![2],
            generation: 1,
        },
        sequence: n * 10,
        change_sequence: n,
        replaced_gap_ids: vec![],
        interval_start: n,
        interval_end: n,
        changes: vec![],
        gaps: vec![],
        trust: Trust::Gap,
    }
}
fn gap(id: u64) -> Gap {
    Gap {
        id,
        reason: GapReason::SignalLost,
        path: None,
        start: Some(1),
        end: Some(2),
    }
}
fn snapshot(id: u8, cut: u64, at: u64) -> Snapshot {
    Snapshot {
        definition: definition(id),
        state: State::Active,
        generation: 1,
        sequence: 10000 + cut,
        last_change_sequence: cut,
        last_observed: Some(at),
        observed_at: at,
        gaps: vec![],
    }
}
#[derive(Default)]
struct Sink {
    fail: bool,
    count: usize,
    independent: Vec<u64>,
    resyncs: usize,
    gaps: BTreeMap<u8, BTreeMap<u64, Gap>>,
}
impl Sink {
    fn apply(&mut self, update: Update<'_>) -> Result<(), &'static str> {
        if self.fail {
            return Err("failed");
        }
        self.count += 1;
        match update {
            Update::Batch { batch, gaps } => {
                self.independent
                    .extend(batch.changes.iter().map(|c| c.last));
                self.gaps.insert(batch.context.watch_ref[0], gaps.clone());
            }
            Update::Snapshot { snapshot, gaps, .. } => {
                self.resyncs += 1;
                self.gaps
                    .insert(snapshot.definition.watch_ref[0], gaps.clone());
            }
            Update::VolumeReplaced { .. } => {}
        }
        Ok(())
    }
}
fn ledger() -> Ledger {
    let mut l = Ledger::new([1; 16]);
    l.register::<()>(definition(3)).unwrap();
    bootstrap(&mut l, 3);
    l
}
fn bootstrap(l: &mut Ledger, id: u8) {
    let mut initial = snapshot(id, 0, 0);
    initial.sequence = 0;
    l.apply_snapshot(&initial, &mut |_| Ok::<(), ()>(()))
        .unwrap();
}
fn apply(l: &mut Ledger, b: &Batch, s: &mut Sink) -> Result<Outcome, Error<&'static str>> {
    l.apply(b, &mut |u| s.apply(u))
}
fn snap(l: &mut Ledger, b: &Snapshot, s: &mut Sink) -> Result<Outcome, Error<&'static str>> {
    l.apply_snapshot(b, &mut |u| s.apply(u))
}
fn change(at: u64) -> Change {
    Change {
        path: vec![b"private".to_vec()],
        subtree: true,
        kinds: BTreeSet::from([ChangeKind::Modify]),
        first: at,
        last: at,
    }
}

#[test]
fn apply_failure_keeps_cursor_and_evidence_until_snapshot_retry() {
    let mut l = ledger();
    let mut sink = Sink::default();
    let mut one = batch(3, 1);
    one.changes.push(change(1));
    one.gaps.push(gap(1));
    assert_eq!(apply(&mut l, &one, &mut sink), Ok(Outcome::Applied));
    let before = l.unit(&[3; 16]).unwrap().clone();
    sink.fail = true;
    assert_eq!(
        apply(&mut l, &batch(3, 2), &mut sink),
        Err(Error::Apply("failed"))
    );
    let after = l.unit(&[3; 16]).unwrap();
    assert_eq!(after.last_change_sequence, before.last_change_sequence);
    assert_eq!(after.gaps, before.gaps);
    assert!(after.needs_resync);
    assert_eq!(sink.independent, vec![1]);
    let mut cut = snapshot(3, 1, 3);
    cut.gaps = vec![gap(1)];
    assert_eq!(snap(&mut l, &cut, &mut sink), Err(Error::Apply("failed")));
    assert_eq!(l.unit(&[3; 16]).unwrap().last_change_sequence, 1);
    sink.fail = false;
    assert_eq!(snap(&mut l, &cut, &mut sink), Ok(Outcome::Applied));
    assert_eq!(apply(&mut l, &batch(3, 2), &mut sink), Ok(Outcome::Applied));
    assert_eq!(sink.independent, vec![1]);
}

#[test]
fn scope_window_count_eviction_preserves_both_units_and_committed_evidence() {
    let mut l = ledger();
    l.register::<()>(definition(4)).unwrap();
    bootstrap(&mut l, 4);
    let mut sink = Sink::default();
    let mut first = batch(3, 1);
    first.changes.push(change(1));
    apply(&mut l, &first, &mut sink).unwrap();
    apply(&mut l, &batch(4, 1), &mut sink).unwrap();
    for n in 2..=1023 {
        apply(&mut l, &batch(3, n), &mut sink).unwrap();
    }
    assert_eq!(l.window.records.len(), 1024);
    apply(&mut l, &batch(4, 2), &mut sink).unwrap();
    assert_eq!(l.window.records.len(), 1024);
    assert_eq!(l.window.records.front().unwrap().unit, [4; 16]);
    let before = l.unit(&[3; 16]).unwrap().clone();
    let other = l.unit(&[4; 16]).unwrap().clone();
    let count = sink.count;
    assert_eq!(
        snap(&mut l, &snapshot(3, 0, 0), &mut sink),
        Ok(Outcome::Resync)
    );
    assert_eq!(
        l.unit(&[3; 16]).unwrap().last_change_sequence,
        before.last_change_sequence
    );
    assert_eq!(l.unit(&[4; 16]), Some(&other));
    assert_eq!(sink.count, count);
    assert_eq!(sink.independent, vec![1]);
}

#[test]
fn byte_limit_exact_overflow_and_single_oversize_are_bounded() {
    use super::replay::{MAX_BYTES, Record, Window};
    let mut w = Window::default();
    let mut b = batch(3, 1);
    b.gaps.push(Gap {
        path: Some(vec![vec![]]),
        ..gap(1)
    });
    let base = Record::estimated_bytes(&b).unwrap();
    b.gaps[0].path.as_mut().unwrap()[0].resize(MAX_BYTES - base, 7);
    assert_eq!(Record::estimated_bytes(&b), Some(MAX_BYTES));
    w.push_batch(&b);
    assert_eq!(w.bytes, MAX_BYTES);
    assert_eq!(w.records.len(), 1);
    w.push_batch(&batch(4, 1));
    assert_eq!(w.records.len(), 1);
    assert_eq!(w.records[0].unit, [4; 16]);
    b.gaps[0].path.as_mut().unwrap()[0].push(7);
    w.push_batch(&b);
    assert_eq!(w.records.len(), 1);
    assert_eq!(w.records[0].unit, [4; 16]);
}

#[test]
fn replay_keeps_only_gap_metadata_and_preserves_committed_changes() {
    let mut l = ledger();
    let mut sink = Sink::default();
    let mut first = batch(3, 1);
    first.gaps.push(gap(1));
    apply(&mut l, &first, &mut sink).unwrap();
    let mut second = batch(3, 2);
    second.gaps.push(gap(2));
    second.changes.push(change(2));
    apply(&mut l, &second, &mut sink).unwrap();
    let mut cut = snapshot(3, 1, 2);
    cut.gaps.push(gap(1));
    assert_eq!(snap(&mut l, &cut, &mut sink), Ok(Outcome::Applied));
    assert_eq!(sink.independent, vec![2]);
    assert!(sink.gaps[&3].contains_key(&2));
    assert_eq!(l.unit(&[3; 16]).unwrap().state, None);
    assert_eq!(snap(&mut l, &cut, &mut sink), Ok(Outcome::Duplicate));
}

#[test]
fn repeated_cut_preserves_last_batch_end_after_replay_records_are_removed() {
    let mut ledger = ledger();
    let mut sink = Sink::default();
    assert_eq!(
        apply(&mut ledger, &batch(3, 1), &mut sink),
        Ok(Outcome::Applied)
    );
    let first = snapshot(3, 1, 2);
    let mut starts = Vec::new();
    assert_eq!(
        ledger.apply_snapshot(&first, &mut |update| {
            if let Update::Snapshot { resync_start, .. } = update {
                starts.push(resync_start);
            }
            Ok::<(), ()>(())
        }),
        Ok(Outcome::Applied)
    );
    assert!(ledger.window.records.is_empty());
    assert_eq!(
        apply(&mut ledger, &batch(3, 2), &mut sink),
        Ok(Outcome::Applied)
    );

    let mut repeated_cut = first.clone();
    repeated_cut.observed_at = 3;
    repeated_cut.last_observed = Some(3);
    assert_eq!(
        ledger.apply_snapshot(&repeated_cut, &mut |_| Err::<(), _>("failed")),
        Err(Error::Apply("failed"))
    );
    assert_eq!(ledger.unit(&[3; 16]).unwrap().last_change_sequence, 2);
    assert_eq!(
        ledger.apply_snapshot(&repeated_cut, &mut |update| {
            if let Update::Snapshot { resync_start, .. } = update {
                starts.push(resync_start);
            }
            Ok::<(), ()>(())
        }),
        Ok(Outcome::Applied)
    );
    assert_eq!(starts, vec![Some(1), Some(1)]);
    assert_eq!(ledger.unit(&[3; 16]).unwrap().last_change_sequence, 2);
}

#[test]
fn release_reclaims_unit_and_scope_without_touching_other_evidence() {
    let mut l = ledger();
    l.register::<()>(definition(4)).unwrap();
    bootstrap(&mut l, 4);
    let mut sink = Sink::default();
    let mut a = batch(3, 1);
    a.changes.push(change(1));
    apply(&mut l, &a, &mut sink).unwrap();
    apply(&mut l, &batch(4, 1), &mut sink).unwrap();
    l.release_unit([3; 16]);
    assert!(l.unit(&[3; 16]).is_none());
    assert!(l.window.records.iter().all(|r| r.unit == [4; 16]));
    assert_eq!(sink.independent, vec![1]);
    assert!(l.unit(&[4; 16]).is_some());
    l.release();
    assert!(l.unit(&[4; 16]).is_none());
    assert_eq!(l.window.bytes, 0);
    assert_eq!(
        apply(&mut l, &batch(4, 2), &mut sink),
        Ok(Outcome::ReleasedScope)
    );
    assert_eq!(sink.independent, vec![1]);
}

#[test]
fn closed_replacement_and_snapshot_order_are_fenced() {
    let mut l = ledger();
    let mut sink = Sink::default();
    let mut first = batch(3, 1);
    first.gaps = vec![gap(1), gap(2)];
    apply(&mut l, &first, &mut sink).unwrap();
    let mut second = batch(3, 2);
    second.gaps.push(gap(3));
    second.replaced_gap_ids = vec![1, 2];
    assert_eq!(apply(&mut l, &second, &mut sink), Ok(Outcome::Applied));
    assert_eq!(apply(&mut l, &second, &mut sink), Ok(Outcome::Duplicate));
    let mut cut = snapshot(3, 2, 5);
    cut.gaps.push(gap(3));
    snap(&mut l, &cut, &mut sink).unwrap();
    let mut older = cut.clone();
    older.observed_at = 4;
    older.last_observed = Some(4);
    assert_eq!(snap(&mut l, &older, &mut sink), Ok(Outcome::Resync));
    assert_eq!(
        l.unit(&[3; 16])
            .unwrap()
            .gaps
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![3]
    );
}

#[test]
fn snapshot_gap_order_does_not_change_identity_but_changed_evidence_does() {
    let mut ledger = ledger();
    let mut sink = Sink::default();
    let mut cut = snapshot(3, 0, 2);
    cut.gaps = vec![gap(1), gap(2)];
    assert_eq!(snap(&mut ledger, &cut, &mut sink), Ok(Outcome::Applied));
    let count = sink.count;

    let mut reordered = cut.clone();
    reordered.gaps.reverse();
    assert_eq!(
        snap(&mut ledger, &reordered, &mut sink),
        Ok(Outcome::Duplicate)
    );
    assert_eq!(sink.count, count);
    assert!(!ledger.unit(&[3; 16]).unwrap().needs_resync);

    reordered.gaps[0].end = Some(3);
    reordered.observed_at = 3;
    assert_eq!(
        snap(&mut ledger, &reordered, &mut sink),
        Ok(Outcome::Resync)
    );
    assert_eq!(sink.count, count);
    assert_eq!(sink.gaps[&3][&2], gap(2));
    assert!(ledger.unit(&[3; 16]).unwrap().needs_resync);
}

#[test]
fn bad_replacements_and_generation_regression_do_not_commit() {
    for replacement in [vec![1, 99], vec![1, 1], vec![2]] {
        let mut l = ledger();
        let mut sink = Sink::default();
        let mut first = batch(3, 1);
        let mut open = gap(2);
        open.end = None;
        first.gaps = vec![gap(1), open];
        assert_eq!(apply(&mut l, &first, &mut sink), Ok(Outcome::Applied));
        let before = l.unit(&[3; 16]).unwrap().clone();
        let mut invalid = batch(3, 2);
        invalid.replaced_gap_ids = replacement;
        invalid.gaps.push(gap(3));
        assert_eq!(apply(&mut l, &invalid, &mut sink), Ok(Outcome::Resync));
        let after = l.unit(&[3; 16]).unwrap();
        assert_eq!(after.gaps, before.gaps);
        assert_eq!(after.last_change_sequence, 1);
        assert_eq!(sink.count, 1);
    }
    let mut l = ledger();
    let mut sink = Sink::default();
    let mut first = batch(3, 1);
    first.context.generation = 2;
    apply(&mut l, &first, &mut sink).unwrap();
    assert_eq!(apply(&mut l, &batch(3, 2), &mut sink), Ok(Outcome::Resync));
    assert_eq!(sink.count, 1);
}

#[test]
fn volume_mismatch_does_not_send_change_ranges_to_sink() {
    let mut l = ledger();
    let mut sink = Sink::default();
    let mut b = batch(3, 1);
    b.context.volume = vec![9];
    b.changes.push(change(1));
    assert_eq!(apply(&mut l, &b, &mut sink), Ok(Outcome::VolumeReplaced));
    assert!(sink.independent.is_empty());
    assert!(l.unit(&[3; 16]).unwrap().volume_unverifiable);
}

#[test]
fn a_new_generation_does_not_inherit_snapshot_status() {
    let mut ledger = Ledger::new([1; 16]);
    ledger.register::<()>(definition(3)).unwrap();
    let mut sink = Sink::default();
    assert_eq!(
        snap(&mut ledger, &snapshot(3, 0, 0), &mut sink),
        Ok(Outcome::Applied)
    );
    assert_eq!(ledger.unit(&[3; 16]).unwrap().state, Some(State::Active));
    let mut newer = batch(3, 1);
    newer.context.generation = 2;
    assert_eq!(apply(&mut ledger, &newer, &mut sink), Ok(Outcome::Applied));
    let unit = ledger.unit(&[3; 16]).unwrap();
    assert_eq!(unit.generation, 2);
    assert_eq!(unit.state, None);
}

#[test]
fn future_cut_then_older_time_or_conflicting_equal_status_requires_resync() {
    let mut l = ledger();
    let mut sink = Sink::default();
    apply(&mut l, &batch(3, 1), &mut sink).unwrap();
    let mut current = snapshot(3, 5, 100);
    current.gaps.push(gap(5));
    assert_eq!(snap(&mut l, &current, &mut sink), Ok(Outcome::Applied));
    let before = sink.gaps.clone();
    let count = sink.count;
    let mut older = current.clone();
    older.observed_at = 50;
    older.last_observed = Some(50);
    older.gaps.clear();
    assert_eq!(snap(&mut l, &older, &mut sink), Ok(Outcome::Resync));
    assert_eq!(sink.gaps, before);
    assert_eq!(sink.count, count);
    assert_eq!(l.unit(&[3; 16]).unwrap().last_change_sequence, 5);
    assert_eq!(snap(&mut l, &current, &mut sink), Ok(Outcome::Resync));
    let mut later = current.clone();
    later.observed_at = 101;
    later.last_observed = Some(101);
    assert_eq!(snap(&mut l, &later, &mut sink), Ok(Outcome::Applied));
    let mut conflicting = later.clone();
    conflicting.observed_at = 102;
    conflicting.gaps.clear();
    assert_eq!(snap(&mut l, &conflicting, &mut sink), Ok(Outcome::Resync));
}

#[test]
fn producer_registry_values_can_be_consumed() {
    use du_scan_engine::watch_lifecycle::{Action, Capability, Registration, Registry, Support};
    let supported = Support::Supported;
    let caps = Capability {
        notification: supported,
        loss: supported,
        interruption: supported,
        path: supported,
        resource_limit: supported,
    };
    let mut producer = Registry::new(4, 4, Default::default()).unwrap();
    let start = producer.start(definition(3), &[2], caps, 1).unwrap();
    let token = start
        .actions
        .iter()
        .find_map(|a| match a {
            Action::Register { token, .. } => Some(*token),
            _ => None,
        })
        .unwrap();
    let events = producer
        .registration(
            [3; 16],
            token,
            1,
            &Registration::Acquired {
                handle: 7,
                uncovered: vec![],
            },
            2,
        )
        .unwrap();
    let mut l = ledger();
    let mut sink = Sink::default();
    for action in events {
        if let Action::Changes(b) = action {
            assert_eq!(apply(&mut l, &b, &mut sink), Ok(Outcome::Applied));
        }
    }
    let cut = producer.incremental_snapshot([3; 16], 3).unwrap();
    assert_eq!(snap(&mut l, &cut, &mut sink), Ok(Outcome::Applied));
    assert_eq!(l.unit(&[3; 16]).unwrap().gaps.len(), 1);
}

mod snapshot_time;

mod source_delivery;
