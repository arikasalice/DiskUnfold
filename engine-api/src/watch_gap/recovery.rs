//! Classifies verified watch ranges for incremental refresh and recovery planning.

/// Compatibility of the result bound to the watch scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseResult {
    Missing,
    RuleUnavailable,
    Compatible,
}

/// Recovery reason supplied by the normalized whole-unit gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WholeUnitGap {
    Unlocatable,
    TargetDisconnected,
}

/// A watch range already normalized by the gap reason and scope owners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeEvidence {
    WholeUnitGap(WholeUnitGap),
    ClosedDirectoryGap,
    Change { include_descendants: bool },
}

/// Stable reason attached to a recovery plan for the whole included target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryReason {
    NoBaseResult,
    RuleChanged,
    /// Mark the target unverifiable and ask the user to reconfigure it.
    VolumeReplaced,
    GapUnlocatable,
    TargetDisconnected,
}

impl RecoveryReason {
    /// Returns the published recovery reason key for the plan row.
    pub fn key(self) -> &'static str {
        match self {
            Self::NoBaseResult => "refresh.no_base_result",
            Self::RuleChanged => "refresh.rule_changed",
            Self::VolumeReplaced => "refresh.volume_replaced",
            Self::GapUnlocatable => "refresh.gap_unlocatable",
            Self::TargetDisconnected => "refresh.target_disconnected",
        }
    }
}

/// The first matching result of the published range decision table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Discard,
    RecoverTarget(RecoveryReason),
    QueueRange { include_descendants: bool },
}

/// Applies the F33 range table in order without widening an excluded range.
///
/// `within_scope` includes target boundaries and exclusion checks. Only trusted
/// owner evidence can establish `volume_matches`. A directory gap is eligible
/// here only after closure. Recovery rows never schedule automatic work.
pub fn decide(
    within_scope: bool,
    base: BaseResult,
    volume_matches: bool,
    evidence: RangeEvidence,
) -> Decision {
    if !within_scope {
        return Decision::Discard;
    }
    match base {
        BaseResult::Missing => return Decision::RecoverTarget(RecoveryReason::NoBaseResult),
        BaseResult::RuleUnavailable => return Decision::RecoverTarget(RecoveryReason::RuleChanged),
        BaseResult::Compatible => {}
    }
    if !volume_matches {
        return Decision::RecoverTarget(RecoveryReason::VolumeReplaced);
    }
    match evidence {
        RangeEvidence::WholeUnitGap(reason) => Decision::RecoverTarget(match reason {
            WholeUnitGap::Unlocatable => RecoveryReason::GapUnlocatable,
            WholeUnitGap::TargetDisconnected => RecoveryReason::TargetDisconnected,
        }),
        RangeEvidence::ClosedDirectoryGap => Decision::QueueRange {
            include_descendants: true,
        },
        RangeEvidence::Change {
            include_descendants,
        } => Decision::QueueRange {
            include_descendants,
        },
    }
}

#[cfg(test)]
mod tests;
