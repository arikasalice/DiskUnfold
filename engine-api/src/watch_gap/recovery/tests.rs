use super::{
    BaseResult, ClassifiedGap, Decision, GapScope, RangeEvidence, RecoveryReason, WholeUnitGap,
    classify_gap, decide,
};
use du_analysis_index::freshness::Settled;
use du_scan_engine::watch_coalescer::{Gap, GapReason};

#[test]
fn classifies_all_published_watch_gap_reasons() {
    use GapReason::{
        BeforeStart, ConsumerResync, Overflow, PartialCapability, SignalLost, Stopped, Unavailable,
    };
    for (reason, state, recovery) in [
        (
            BeforeStart,
            Settled::GapUnknown,
            Some(WholeUnitGap::Unlocatable),
        ),
        (
            Overflow,
            Settled::GapUnknown,
            Some(WholeUnitGap::Unlocatable),
        ),
        (
            ConsumerResync,
            Settled::GapUnknown,
            Some(WholeUnitGap::Unlocatable),
        ),
        (
            SignalLost,
            Settled::GapUnknown,
            Some(WholeUnitGap::Unlocatable),
        ),
        (
            Unavailable,
            Settled::GapUnknown,
            Some(WholeUnitGap::TargetDisconnected),
        ),
        (Stopped, Settled::Unwatched, None),
        (PartialCapability, Settled::Unwatched, None),
    ] {
        let gap = Gap {
            id: 1,
            reason,
            path: None,
            start: Some(3),
            end: None,
        };
        assert_eq!(
            classify_gap(&gap),
            ClassifiedGap {
                scope: GapScope::Unit,
                state,
                range_evidence: recovery.map(RangeEvidence::WholeUnitGap),
            }
        );
    }
}

#[test]
fn directory_loss_keeps_its_path_and_only_queues_after_closure() {
    for path in [vec![], vec![b"nested".to_vec()]] {
        for end in [None, Some(4)] {
            let gap = Gap {
                id: 1,
                reason: GapReason::SignalLost,
                path: Some(path.clone()),
                start: Some(3),
                end,
            };
            let classified = classify_gap(&gap);
            assert_eq!(classified.scope, GapScope::Directory(&path));
            assert_eq!(classified.state, Settled::GapUnknown);
            assert_eq!(
                classified.range_evidence,
                end.map(|_| RangeEvidence::ClosedDirectoryGap)
            );
        }
    }
}

#[test]
fn first_matching_scope_or_compatibility_failure_wins() {
    use BaseResult::{Compatible, Missing, RuleUnavailable};
    use Decision::{Discard, RecoverTarget};

    let gap = RangeEvidence::WholeUnitGap(WholeUnitGap::TargetDisconnected);
    let cases = [
        (false, Missing, false, Discard),
        (false, Compatible, true, Discard),
        (
            true,
            Missing,
            false,
            RecoverTarget(RecoveryReason::NoBaseResult),
        ),
        (
            true,
            RuleUnavailable,
            false,
            RecoverTarget(RecoveryReason::RuleChanged),
        ),
        (
            true,
            Compatible,
            false,
            RecoverTarget(RecoveryReason::VolumeReplaced),
        ),
    ];
    for (within_scope, base, volume_matches, expected) in cases {
        assert_eq!(decide(within_scope, base, volume_matches, gap), expected);
    }
}

#[test]
fn whole_unit_gaps_create_plan_rows_without_queueing() {
    for (gap, reason, key) in [
        (
            WholeUnitGap::Unlocatable,
            RecoveryReason::GapUnlocatable,
            "refresh.gap_unlocatable",
        ),
        (
            WholeUnitGap::TargetDisconnected,
            RecoveryReason::TargetDisconnected,
            "refresh.target_disconnected",
        ),
    ] {
        assert_eq!(
            decide(
                true,
                BaseResult::Compatible,
                true,
                RangeEvidence::WholeUnitGap(gap)
            ),
            Decision::RecoverTarget(reason)
        );
        assert_eq!(reason.key(), key);
    }
    assert_eq!(RecoveryReason::NoBaseResult.key(), "refresh.no_base_result");
    assert_eq!(RecoveryReason::RuleChanged.key(), "refresh.rule_changed");
    assert_eq!(
        RecoveryReason::VolumeReplaced.key(),
        "refresh.volume_replaced"
    );
}

#[test]
fn only_a_closed_directory_gap_widens_the_queue_to_its_descendants() {
    for (evidence, expected) in [
        (RangeEvidence::ClosedDirectoryGap, true),
        (
            RangeEvidence::Change {
                include_descendants: false,
            },
            false,
        ),
        (
            RangeEvidence::Change {
                include_descendants: true,
            },
            true,
        ),
    ] {
        assert_eq!(
            decide(true, BaseResult::Compatible, true, evidence),
            Decision::QueueRange {
                include_descendants: expected
            }
        );
        assert_eq!(
            decide(false, BaseResult::Compatible, true, evidence),
            Decision::Discard
        );
    }
}
