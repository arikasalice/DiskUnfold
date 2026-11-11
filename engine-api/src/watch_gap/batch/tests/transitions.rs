use super::*;
use crate::watch_gap::Error;

fn snapshot(cut: u64, at: u64, gaps: Vec<Gap>) -> Snapshot {
    Snapshot {
        definition: definition(1),
        state: State::Active,
        generation: 1,
        sequence: cut + at,
        last_change_sequence: cut,
        last_observed: Some(at),
        observed_at: at,
        gaps,
    }
}
fn batch(n: u64, gaps: Vec<Gap>, replaced_gap_ids: Vec<u64>) -> Batch {
    Batch {
        context: Context {
            watch_ref: [3; 16],
            scope_revision: [1; 16],
            volume: vec![2],
            generation: 1,
        },
        sequence: n,
        change_sequence: n,
        interval_start: n,
        interval_end: n,
        changes: vec![],
        gaps,
        replaced_gap_ids,
        trust: Trust::Gap,
    }
}
fn gap(id: u64, end: Option<u64>) -> Gap {
    Gap {
        id,
        reason: GapReason::SignalLost,
        path: Some(vec![b"dir".to_vec()]),
        start: Some(0),
        end,
    }
}
fn apply(l: &mut Ledger, b: &Batch) -> (Outcome, Option<Plan<u8, Vec<u8>>>) {
    let mut plan = None;
    let outcome = l
        .apply(b, &mut |u| {
            plan = prepare([1; 16], u, &mut Owner, 256);
            Ok::<(), ()>(())
        })
        .unwrap();
    (outcome, plan)
}
fn snap(l: &mut Ledger, s: &Snapshot) -> Plan<u8, Vec<u8>> {
    let mut plan = None;
    l.apply_snapshot(s, &mut |u| {
        plan = prepare([1; 16], u, &mut Owner, 256);
        Ok::<(), ()>(())
    })
    .unwrap();
    plan.unwrap()
}
fn ledger() -> Ledger {
    let mut l = Ledger::new([1; 16]);
    l.register::<()>(definition(1)).unwrap();
    l
}

#[test]
fn only_new_and_newly_closed_gaps_enter_once_not_summaries_or_snapshots() {
    let mut l = ledger();
    assert!(snap(&mut l, &snapshot(0, 0, vec![])).automatic.is_empty());
    let (_, open) = apply(&mut l, &batch(1, vec![gap(1, None)], vec![]));
    assert!(open.unwrap().automatic.is_empty());
    let (_, closed) = apply(&mut l, &batch(2, vec![gap(1, Some(2))], vec![]));
    assert_eq!(closed.unwrap().automatic.len(), 1);
    let (_, repeated) = apply(&mut l, &batch(3, vec![gap(1, Some(2))], vec![]));
    assert!(repeated.unwrap().automatic.is_empty());
    let (_, normal) = apply(&mut l, &batch(4, vec![], vec![]));
    assert!(normal.unwrap().automatic.is_empty());
    let (_, summary) = apply(&mut l, &batch(5, vec![gap(2, Some(2))], vec![1]));
    assert!(summary.unwrap().automatic.is_empty());
    assert!(
        snap(&mut l, &snapshot(5, 6, vec![gap(2, Some(2))]))
            .automatic
            .is_empty()
    );
    let (_, fresh) = apply(&mut l, &batch(6, vec![gap(3, Some(6))], vec![]));
    assert_eq!(fresh.unwrap().automatic.len(), 1);
}

#[test]
fn sequence_gap_and_resync_snapshot_do_not_requeue_old_evidence() {
    let mut l = ledger();
    snap(&mut l, &snapshot(0, 0, vec![gap(1, Some(0))]));
    let (outcome, plan) = apply(&mut l, &batch(2, vec![], vec![]));
    assert_eq!(outcome, Outcome::Resync);
    assert!(plan.is_none());
    let resync = snap(&mut l, &snapshot(2, 3, vec![gap(1, Some(0))]));
    assert!(resync.automatic.is_empty());
    assert!(
        apply(&mut l, &batch(3, vec![], vec![]))
            .1
            .unwrap()
            .automatic
            .is_empty()
    );
}

#[test]
fn replaced_volume_cannot_be_rebound_to_another_scope() {
    let mut l = ledger();
    snap(&mut l, &snapshot(0, 0, vec![]));
    let mut changed = batch(1, vec![], vec![]);
    changed.context.volume = vec![9];
    let mut right = None;
    let result = l.apply(&changed, &mut |update| {
        if let Update::VolumeReplaced { scope, watch_ref } = update {
            assert!(
                prepare(
                    [2; 16],
                    Update::VolumeReplaced { scope, watch_ref },
                    &mut Owner,
                    256
                )
                .is_none()
            );
            right = prepare(
                [1; 16],
                Update::VolumeReplaced { scope, watch_ref },
                &mut Owner,
                256,
            );
        }
        Ok::<(), ()>(())
    });
    assert_eq!(result, Ok::<Outcome, Error<()>>(Outcome::VolumeReplaced));
    let p = right.unwrap();
    assert!(p.automatic.is_empty());
    assert_eq!(p.recovery, vec![(3, RecoveryReason::VolumeReplaced)]);
}

#[test]
fn full_recovery_evidence_remains_visible_without_requeueing_old_gaps() {
    let mut l = ledger();
    snap(&mut l, &snapshot(0, 0, vec![]));
    let loss = Gap {
        id: 7,
        reason: GapReason::Overflow,
        path: None,
        start: Some(0),
        end: Some(1),
    };
    assert_eq!(
        apply(&mut l, &batch(1, vec![loss], vec![]))
            .1
            .unwrap()
            .recovery,
        vec![(3, RecoveryReason::GapUnlocatable)]
    );
    let mut next = batch(2, vec![], vec![]);
    next.changes.push(change(&["new"], false));
    let p = apply(&mut l, &next).1.unwrap();
    assert_eq!(p.recovery, vec![(3, RecoveryReason::GapUnlocatable)]);
    assert_eq!(
        p.automatic.get(&3),
        Some(&Selection::Directories(vec![Directory {
            components: vec![b"new".to_vec()],
            include_descendants: false
        }]))
    );
}

#[test]
fn disconnected_bound_target_remains_in_recovery_without_verified_current_volume() {
    let mut consumer = ledger();
    snap(&mut consumer, &snapshot(0, 0, vec![]));
    let unavailable = Gap {
        id: 7,
        reason: GapReason::Unavailable,
        path: None,
        start: Some(0),
        end: None,
    };
    let mut next = batch(1, vec![unavailable], vec![]);
    next.changes.push(change(&["volume"], true));
    let plan = apply(&mut consumer, &next).1.unwrap();
    assert!(plan.automatic.is_empty());
    assert_eq!(plan.recovery, vec![(3, RecoveryReason::TargetDisconnected)]);
}

#[test]
fn recovery_is_separate_from_the_known_prior_time() {
    let mut l = ledger();
    snap(&mut l, &snapshot(0, 0, vec![]));
    let (outcome, _) = apply(&mut l, &batch(2, vec![], vec![]));
    assert_eq!(outcome, Outcome::Resync);
    let mut observed = None;
    l.apply_snapshot(&snapshot(2, 3, vec![]), &mut |u| {
        if let Update::Snapshot {
            resync_required,
            resync_start,
            ..
        } = &u
        {
            observed = Some((*resync_required, *resync_start));
        }
        let plan = prepare([1; 16], u, &mut Owner, 256).unwrap();
        assert_eq!(plan.recovery, vec![(3, RecoveryReason::GapUnlocatable)]);
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(observed, Some((true, None)));

    let mut normal = ledger();
    snap(&mut normal, &snapshot(0, 0, vec![]));
    apply(&mut normal, &batch(1, vec![], vec![]));
    let mut observed = None;
    normal
        .apply_snapshot(&snapshot(1, 2, vec![]), &mut |u| {
            if let Update::Snapshot {
                resync_required,
                resync_start,
                ..
            } = &u
            {
                observed = Some((*resync_required, *resync_start));
            }
            assert!(
                prepare([1; 16], u, &mut Owner, 256)
                    .unwrap()
                    .recovery
                    .is_empty()
            );
            Ok::<(), ()>(())
        })
        .unwrap();
    assert_eq!(observed, Some((false, Some(1))));
}

#[test]
fn advancing_snapshot_without_observed_batch_requires_recovery() {
    let mut l = ledger();
    snap(&mut l, &snapshot(0, 0, vec![]));
    let plan = snap(&mut l, &snapshot(1, 2, vec![]));
    assert_eq!(plan.recovery, vec![(3, RecoveryReason::GapUnlocatable)]);
}

#[test]
fn gap_evidence_is_admitted_once_per_update() {
    struct CountingOwner(usize);
    impl ScopeOwner for CountingOwner {
        type Target = u8;
        type Component = Vec<u8>;
        fn admit(
            &mut self,
            scope: [u8; 16],
            watch: [u8; 16],
            path: Option<&Key>,
        ) -> Option<Admitted<u8, Vec<u8>>> {
            self.0 += 1;
            Owner.admit(scope, watch, path)
        }
    }
    let mut l = ledger();
    snap(&mut l, &snapshot(0, 0, vec![]));
    let mut owner = CountingOwner(0);
    l.apply(&batch(1, vec![gap(1, Some(1))], vec![]), &mut |u| {
        let plan = prepare([1; 16], u, &mut owner, 256).unwrap();
        assert_eq!(plan.automatic.len(), 1);
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(owner.0, 1);
}
