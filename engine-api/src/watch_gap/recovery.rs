//! Classifies verified watch ranges for incremental refresh and recovery planning.

use du_analysis_index::freshness::Settled;
use du_scan_engine::watch_coalescer::{Gap, GapReason, Key};

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

/// Scope for the freshness evidence, independent of its refresh eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapScope<'a> {
    Unit,
    Directory(&'a Key),
}

/// The published gap classification before applying scope and base-result checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassifiedGap<'a> {
    pub scope: GapScope<'a>,
    pub state: Settled,
    /// `None` leaves the freshness evidence in place without scheduling work.
    pub range_evidence: Option<RangeEvidence>,
}

/// Maps an F18 gap to its F33 freshness scope and possible recovery input.
///
/// Only a closed loss with a verified directory can enter automatic refresh.
/// Use the original gap for the observation boundaries and replacement identity.
pub fn classify_gap(gap: &Gap) -> ClassifiedGap<'_> {
    use GapReason::{
        BeforeStart, ConsumerResync, Overflow, PartialCapability, SignalLost, Stopped, Unavailable,
    };
    use GapScope::{Directory, Unit};
    use RangeEvidence::{ClosedDirectoryGap, WholeUnitGap as Recover};
    use Settled::{GapUnknown, Unwatched};

    let (scope, state, range_evidence) = match (gap.reason, gap.path.as_ref()) {
        (SignalLost, Some(path)) => (
            Directory(path),
            GapUnknown,
            gap.end.map(|_| ClosedDirectoryGap),
        ),
        (BeforeStart | Overflow | ConsumerResync | SignalLost, _) => {
            (Unit, GapUnknown, Some(Recover(WholeUnitGap::Unlocatable)))
        }
        (Unavailable, _) => (
            Unit,
            GapUnknown,
            Some(Recover(WholeUnitGap::TargetDisconnected)),
        ),
        (Stopped | PartialCapability, _) => (Unit, Unwatched, None),
    };
    ClassifiedGap {
        scope,
        state,
        range_evidence,
    }
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
