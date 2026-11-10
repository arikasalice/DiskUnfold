#![allow(clippy::panic)]
use super::*;

fn context() -> Context {
    Context {
        watch_ref: [1; 16],
        scope_revision: [2; 16],
        volume: vec![3],
        generation: 7,
    }
}
fn open_gap() -> Gap {
    Gap {
        id: 4,
        reason: GapReason::PartialCapability,
        path: None,
        start: Some(2),
        end: None,
    }
}
fn config() -> Config {
    Config {
        window_ms: 1000,
        promote_at: 64,
        queue_limit: 4096,
        gap_limit: 16,
    }
}
fn stream(cfg: Config, sequence: u64, gaps: Vec<Gap>) -> Coalescer {
    match Coalescer::new(
        context(),
        cfg,
        Cursor {
            sequence,
            change_sequence: sequence,
            last_gap_id: 4,
        },
        gaps,
    ) {
        Ok(v) => v,
        Err(e) => panic!("invalid test setup: {e:?}"),
    }
}
fn key(path: &[&str]) -> Key {
    path.iter().map(|s| s.as_bytes().to_vec()).collect()
}
fn signal(path: &[&str], kind: ChangeKind, directory_subtree: bool, time: u64) -> Signal {
    Signal {
        path: Some(key(path)),
        kind,
        directory_subtree,
        observed_at: time,
    }
}
#[test]
fn overflow_gap_budget_rejection_is_atomic_and_retriable() {
    let mut cfg = config();
    cfg.queue_limit = 2;
    cfg.gap_limit = 1;
    let mut s = stream(cfg, 9, vec![open_gap()]);
    let first = signal(&["a", "f"], ChangeKind::Create, false, 10);
    let second = signal(&["b", "f"], ChangeKind::Delete, false, 11);
    assert_eq!(s.signal(7, &first), Ok(vec![]));
    assert_eq!(
        s.signal(7, &second),
        Err(Error::GapBudget {
            required: 2,
            available: 1
        })
    );
    assert_eq!((s.sequence(), s.buffered_len()), (9, 1));
    assert_eq!(s.set_gap_limit(2), Ok(()));
    let Ok(out) = s.signal(7, &second) else {
        panic!("retry must succeed")
    };
    let [batch] = &out[..] else {
        panic!("one overflow batch")
    };
    assert_eq!(batch.context, context());
    assert_eq!(batch.sequence, 10);
    assert_eq!((batch.interval_start, batch.interval_end), (10, 11));
    assert_eq!(batch.trust, Trust::Gap);
    assert!(batch.changes.is_empty());
    let gap = &batch.gaps[0];
    assert_eq!(
        (gap.id, gap.reason, &gap.path, gap.start, gap.end),
        (5, GapReason::Overflow, &None, Some(10), Some(11))
    );
    assert_eq!(batch.gaps[1], open_gap());
    assert_eq!(s.buffered_len(), 0);
}
#[test]
fn loss_budget_rejection_preserves_evidence_and_sequence() {
    let mut cfg = config();
    cfg.gap_limit = 1;
    let mut s = stream(cfg, 2, vec![open_gap()]);
    assert!(
        s.signal(7, &signal(&["a", "f"], ChangeKind::Modify, false, 4))
            .is_ok()
    );
    assert_eq!(
        s.signal_lost(7, None, 5),
        Err(Error::GapBudget {
            required: 2,
            available: 1
        })
    );
    assert_eq!((s.sequence(), s.buffered_len()), (2, 1));
    assert!(s.set_gap_limit(2).is_ok());
    let Ok(out) = s.signal_lost(7, None, 5) else {
        panic!("retry")
    };
    assert_eq!(out[0].changes.len(), 1);
    assert_eq!(out[0].gaps[0].reason, GapReason::SignalLost);
    assert_eq!(
        (out[0].gaps[0].start, out[0].gaps[0].end),
        (Some(4), Some(5))
    );
}
#[test]
fn sorted_split_batches_preserve_context_evidence_and_sequence() {
    let mut s = stream(config(), 100, vec![open_gap()]);
    for n in (0..33).rev() {
        let item = Signal {
            path: Some(vec![vec![n], b"f".to_vec()]),
            directory_subtree: false,
            kind: ChangeKind::Modify,
            observed_at: 30,
        };
        assert!(s.signal(7, &item).is_ok());
    }
    assert_eq!(s.tick(7, 1029), Ok(vec![]));
    let Ok(out) = s.tick(7, 1030) else {
        panic!("flush")
    };
    let [a, b] = &out[..] else { panic!("split") };
    assert_eq!((a.changes.len(), b.changes.len()), (32, 1));
    assert_eq!((a.sequence, b.sequence), (101, 102));
    assert_eq!(a.context, context());
    assert_eq!(b.context, context());
    assert_eq!((a.interval_start, a.interval_end), (30, 1030));
    assert_eq!((a.trust, b.trust), (Trust::Gap, Trust::Gap));
    assert_eq!(a.gaps, vec![open_gap()]);
    assert!(b.gaps.is_empty());
    assert!(a.changes.windows(2).all(|pair| pair[0].path < pair[1].path));
    assert!(a.changes[31].path < b.changes[0].path);
}
#[test]
fn create_delete_and_move_hints_are_preserved() {
    let mut s = stream(config(), 0, vec![]);
    for item in [
        signal(&["a", "file"], ChangeKind::Create, false, 1),
        signal(&["a", "file"], ChangeKind::Delete, false, 2),
        signal(&["a", "old"], ChangeKind::Rename, true, 3),
        signal(&["b", "new"], ChangeKind::Rename, true, 4),
    ] {
        assert!(s.signal(7, &item).is_ok());
    }
    let Ok(out) = s.tick(7, 1001) else {
        panic!("flush")
    };
    let changes = &out[0].changes;
    let Some(a) = changes.iter().find(|c| c.path == key(&["a"])) else {
        panic!("a")
    };
    assert!(
        a.kinds.contains(&ChangeKind::Create)
            && a.kinds.contains(&ChangeKind::Delete)
            && a.kinds.contains(&ChangeKind::Rename)
    );
    assert!(changes.iter().any(|c| c.path == key(&["a", "old"])
        && c.subtree
        && c.kinds.contains(&ChangeKind::Rename)));
    assert!(changes.iter().any(|c| c.path == key(&["b", "new"])
        && c.subtree
        && c.kinds.contains(&ChangeKind::Rename)));
}
#[test]
fn stale_generation_and_regressing_time_leave_state_untouched() {
    let mut s = stream(config(), 4, vec![]);
    assert_eq!(
        s.signal(
            6,
            &signal(&["ignored", "f"], ChangeKind::Delete, false, 9999)
        ),
        Ok(vec![])
    );
    assert_eq!(s.tick(6, 9999), Ok(vec![]));
    assert_eq!(
        s.signal(7, &signal(&["a", "f"], ChangeKind::Modify, false, 1)),
        Ok(vec![])
    );
    assert_eq!(s.tick(7, 0), Err(Error::ClockRegressed));
    assert_eq!((s.sequence(), s.buffered_len()), (4, 1));
    assert!(s.tick(7, 1001).is_ok());
}
#[test]
fn promotion_and_root_signal_do_not_duplicate_ranges() {
    let mut cfg = config();
    cfg.promote_at = 2;
    let mut s = stream(cfg, 0, vec![]);
    assert!(
        s.signal(7, &signal(&["a", "x", "f"], ChangeKind::Create, false, 1))
            .is_ok()
    );
    assert!(
        s.signal(7, &signal(&["a", "y", "f"], ChangeKind::Delete, false, 2))
            .is_ok()
    );
    let Ok(out) = s.flush(7, 3) else {
        panic!("flush")
    };
    assert_eq!(out[0].changes.len(), 1);
    let c = &out[0].changes[0];
    assert_eq!(c.path, key(&["a"]));
    assert!(c.subtree);
    assert!(c.kinds.contains(&ChangeKind::Create) && c.kinds.contains(&ChangeKind::Delete));
    let root = Signal {
        path: None,
        kind: ChangeKind::Modify,
        directory_subtree: false,
        observed_at: 4,
    };
    assert!(s.signal(7, &root).is_ok());
    assert!(
        s.signal(7, &signal(&["b", "f"], ChangeKind::Delete, false, 5))
            .is_ok()
    );
    let Ok(out) = s.flush(7, 6) else {
        panic!("flush")
    };
    assert_eq!(out[0].changes.len(), 1);
    assert!(out[0].changes[0].path.is_empty());
    assert!(out[0].changes[0].subtree);
}
#[test]
fn status_and_failed_sequence_reservation_do_not_wrap() {
    let mut s = stream(config(), u64::MAX - 1, vec![]);
    assert_eq!(s.reserve_status_sequence(), Ok(u64::MAX));
    assert_eq!(s.reserve_status_sequence(), Err(Error::SequenceExhausted));
    assert!(
        s.signal(7, &signal(&["a", "f"], ChangeKind::Modify, false, 1))
            .is_ok()
    );
    assert_eq!(s.tick(7, 1001), Err(Error::SequenceExhausted));
    assert_eq!((s.sequence(), s.buffered_len()), (u64::MAX, 1));
}
#[test]
fn loss_at_window_boundary_flushes_prior_window_before_gap() {
    let mut s = stream(config(), 0, vec![]);
    assert!(
        s.signal(7, &signal(&["a", "f"], ChangeKind::Modify, false, 10))
            .is_ok()
    );
    let Ok(out) = s.signal_lost(7, None, 1010) else {
        panic!("loss")
    };
    let [first, second] = &out[..] else {
        panic!("two ordered batches")
    };
    assert_eq!(first.sequence, 1);
    assert_eq!(first.trust, Trust::Normal);
    assert_eq!(first.changes.len(), 1);
    assert_eq!(second.sequence, 2);
    assert_eq!(second.trust, Trust::Gap);
    assert!(second.changes.is_empty());
    assert_eq!(second.gaps[0].id, 5);
    assert_eq!(s.last_gap_id(), 5);
}
#[test]
fn loss_after_elapsed_window_does_not_publish_anything_on_budget_failure() {
    let mut cfg = config();
    cfg.gap_limit = 1;
    let mut s = stream(cfg, 0, vec![open_gap()]);
    assert!(
        s.signal(7, &signal(&["a", "f"], ChangeKind::Modify, false, 10))
            .is_ok()
    );
    assert_eq!(
        s.signal_lost(7, None, 1010),
        Err(Error::GapBudget {
            required: 2,
            available: 1
        })
    );
    assert_eq!((s.sequence(), s.buffered_len(), s.last_gap_id()), (0, 1, 4));
    assert!(s.set_gap_limit(2).is_ok());
    let Ok(out) = s.signal_lost(7, None, 1010) else {
        panic!("retry")
    };
    assert_eq!(out.len(), 2);
    assert_eq!((out[0].sequence, out[1].sequence), (1, 2));
}
#[test]
fn split_sequence_exhaustion_is_atomic() {
    let mut s = stream(config(), u64::MAX - 1, vec![]);
    for n in 0..33 {
        let item = Signal {
            path: Some(vec![vec![n], b"f".to_vec()]),
            directory_subtree: false,
            kind: ChangeKind::Modify,
            observed_at: 1,
        };
        assert!(s.signal(7, &item).is_ok());
    }
    assert_eq!(s.tick(7, 1001), Err(Error::SequenceExhausted));
    assert_eq!(s.sequence(), u64::MAX - 1);
    assert_eq!(s.buffered_len(), 33);
}
#[test]
fn same_window_in_different_arrival_orders_has_same_output() {
    let signals = [
        signal(&["z", "a"], ChangeKind::Create, false, 10),
        signal(&["a", "b"], ChangeKind::Modify, false, 10),
        signal(&["z", "c"], ChangeKind::Delete, false, 10),
    ];
    let mut left = stream(config(), 0, vec![]);
    let mut right = stream(config(), 0, vec![]);
    for item in &signals {
        assert!(left.signal(7, item).is_ok());
    }
    for item in signals.iter().rev() {
        assert!(right.signal(7, item).is_ok());
    }
    assert_eq!(left.tick(7, 1010), right.tick(7, 1010));
}
#[test]
fn loss_path_is_borrowed_for_retry_and_emitted_as_subtree() {
    let mut cfg = config();
    cfg.gap_limit = 1;
    let mut s = stream(cfg, 0, vec![open_gap()]);
    let path = key(&["affected"]);
    assert!(matches!(
        s.signal_lost(7, Some(&path), 7),
        Err(Error::GapBudget { .. })
    ));
    assert!(s.set_gap_limit(2).is_ok());
    let Ok(out) = s.signal_lost(7, Some(&path), 7) else {
        panic!("retry")
    };
    assert_eq!(out[0].gaps[0].path.as_ref(), Some(&path));
}
#[test]
fn ordinary_insert_work_depends_on_depth_not_buffer_size() {
    use super::buffer::Buffer;
    let mut cfg = config();
    cfg.queue_limit = 10_000;
    cfg.promote_at = 10_000;
    let mut small = Buffer::default();
    let mut large = Buffer::default();
    for n in 0..4000_u32 {
        let path = vec![n.to_be_bytes().to_vec(), b"dir".to_vec(), b"file".to_vec()];
        let item = Signal {
            path: Some(path),
            directory_subtree: false,
            kind: ChangeKind::Modify,
            observed_at: 10,
        };
        large.push(&item, cfg);
        if n == 0 {
            small.push(&item, cfg);
        }
    }
    let probe = Signal {
        path: Some(vec![
            b"different".to_vec(),
            b"dir".to_vec(),
            b"file".to_vec(),
        ]),
        directory_subtree: false,
        kind: ChangeKind::Modify,
        observed_at: 10,
    };
    small.push(&probe, cfg);
    large.push(&probe, cfg);
    assert_eq!(small.last_work(), 2);
    assert_eq!(large.last_work(), 2);
    assert_eq!(large.len(), 4001);
}
#[test]
fn prefix_removal_visits_only_affected_keys_and_undo_restores_them() {
    use super::buffer::Buffer;
    let mut cfg = config();
    cfg.queue_limit = 10_000;
    cfg.promote_at = 10_000;
    let mut buffer = Buffer::default();
    for n in 0..100_u32 {
        for prefix in [b"a".to_vec(), b"z".to_vec()] {
            let item = Signal {
                path: Some(vec![prefix, n.to_be_bytes().to_vec(), b"file".to_vec()]),
                directory_subtree: false,
                kind: ChangeKind::Modify,
                observed_at: 1,
            };
            buffer.push(&item, cfg);
        }
    }
    let mut original = Buffer::default();
    for n in 0..100_u32 {
        for prefix in [b"a".to_vec(), b"z".to_vec()] {
            let item = Signal {
                path: Some(vec![prefix, n.to_be_bytes().to_vec(), b"file".to_vec()]),
                directory_subtree: false,
                kind: ChangeKind::Modify,
                observed_at: 1,
            };
            original.push(&item, cfg);
        }
    }
    let target = Signal {
        path: Some(vec![b"a".to_vec()]),
        directory_subtree: true,
        kind: ChangeKind::Delete,
        observed_at: 2,
    };
    let undo = buffer.push(&target, cfg);
    // The parent root plus the affected 100 descendants; the 100 z keys are untouched.
    assert_eq!(buffer.last_work(), 101);
    assert_eq!(buffer.len(), 102);
    buffer.rollback(undo);
    assert_eq!(buffer.take(), original.take());
}
#[test]
fn rollback_restores_parent_counts_after_promotion() {
    use super::buffer::Buffer;
    let mut cfg = config();
    cfg.promote_at = 2;
    let mut actual = Buffer::default();
    let mut expected = Buffer::default();
    let first = signal(&["a", "x", "file"], ChangeKind::Create, false, 1);
    let second = signal(&["a", "y", "file"], ChangeKind::Delete, false, 2);
    actual.push(&first, cfg);
    expected.push(&first, cfg);
    let undo = actual.push(&second, cfg);
    assert_eq!(actual.len(), 1);
    actual.rollback(undo);
    actual.push(&second, cfg);
    expected.push(&second, cfg);
    assert_eq!(actual.take(), expected.take());
}
#[test]
fn compaction_handoff_flushes_buffer_before_summary_without_losing_order() {
    let mut s = stream(config(), 7, vec![]);
    assert!(
        s.signal(7, &signal(&["a", "f"], ChangeKind::Modify, false, 1))
            .is_ok()
    );
    let summary = Gap {
        id: 5,
        reason: GapReason::Overflow,
        path: None,
        start: Some(1),
        end: Some(2),
    };
    let Ok(out) = s.compacted_gap(&summary, &[1, 2], 2) else {
        panic!("compaction")
    };
    let [first, second] = &out[..] else {
        panic!("ordered batches")
    };
    assert_eq!((first.sequence, second.sequence), (8, 9));
    assert_eq!(first.changes.len(), 1);
    assert_eq!(second.gaps, vec![summary]);
    assert_eq!((s.sequence(), s.last_gap_id(), s.buffered_len()), (9, 5, 0));
}
#[test]
fn compaction_preflight_failure_preserves_buffer_and_gap_counter() {
    let mut s = stream(config(), u64::MAX - 1, vec![]);
    assert!(
        s.signal(7, &signal(&["a", "f"], ChangeKind::Modify, false, 1))
            .is_ok()
    );
    let summary = Gap {
        id: 5,
        reason: GapReason::Overflow,
        path: None,
        start: Some(1),
        end: Some(2),
    };
    assert_eq!(
        s.compacted_gap(&summary, &[1, 2], 2),
        Err(Error::SequenceExhausted)
    );
    assert_eq!(
        (s.sequence(), s.last_gap_id(), s.buffered_len()),
        (u64::MAX - 1, 4, 1)
    );
}
