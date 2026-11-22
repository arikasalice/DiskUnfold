//! Plan readiness decisions (FR4.HD1.app-host.DD1).
//! The F10 plan builder calls this after its items and estimates are fixed. The sample
//! source row remains isolated in [`readiness_first_row`] for reuse by its caller.

use crate::ports::{PlanItem, PlanItemState};
use du_core_contracts::data_source::DataSource;
use serde::{Deserialize, Serialize};

/// Readiness outcome produced by a row of the F10 readiness table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanReadiness {
    /// The plan was built from sample data and can only be reviewed, never approved.
    SamplePlan,
    /// Snapshot results are historical and cannot be used as current plan input.
    SourceNotCurrent,
    /// At least one item still needs a user's confirmation.
    NeedsConfirmation,
    /// No item is included for execution.
    NoTarget,
    /// The plan is ready to be submitted for approval.
    Ready,
}

/// Evaluates the first row of the F10 readiness decision table.
///
/// An absent or sample source yields [`PlanReadiness::SamplePlan`]. A snapshot
/// source yields [`PlanReadiness::SourceNotCurrent`], and a real source returns
/// `None` so the caller can continue with the next row.
pub fn readiness_first_row(source: Option<DataSource>) -> Option<PlanReadiness> {
    match DataSource::or_sample(source) {
        DataSource::Real => None,
        DataSource::Sample => Some(PlanReadiness::SamplePlan),
        DataSource::Snapshot => Some(PlanReadiness::SourceNotCurrent),
    }
}

/// Evaluates the remaining F10 readiness rows after the sample-data row.
pub(crate) fn evaluate(source: DataSource, items: &[PlanItem]) -> PlanReadiness {
    if let Some(readiness) = readiness_first_row(Some(source)) {
        return readiness;
    }
    if items.iter().any(|item| item.state == PlanItemState::Held) {
        return PlanReadiness::NeedsConfirmation;
    }
    if !items
        .iter()
        .any(|item| item.state == PlanItemState::Included)
    {
        return PlanReadiness::NoTarget;
    }
    PlanReadiness::Ready
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_row_stops_at_sample_or_unknown_sources() {
        let rows = [
            (None, Some(PlanReadiness::SamplePlan)),
            (Some(DataSource::Sample), Some(PlanReadiness::SamplePlan)),
            (Some(DataSource::Real), None),
            (
                Some(DataSource::Snapshot),
                Some(PlanReadiness::SourceNotCurrent),
            ),
        ];
        for (source, expected) in rows {
            assert_eq!(readiness_first_row(source), expected, "{source:?}");
        }
    }
}
