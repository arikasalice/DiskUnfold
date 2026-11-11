use super::{BaseResult, Decision, RangeEvidence, RecoveryReason, WholeUnitGap, decide};

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
