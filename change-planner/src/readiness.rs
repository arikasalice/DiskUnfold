//! Plan readiness decisions (FR4.HD1.app-host.DD1).

use du_core_contracts::data_source::DataSource;

/// Readiness outcome produced by a row of the F10 readiness table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanReadiness {
    /// The plan was built from sample data and can only be reviewed, never approved.
    SamplePlan,
}

/// Evaluates the first row of the F10 readiness decision table.
///
/// An absent or sample source ends the evaluation with [`PlanReadiness::SamplePlan`].
/// A real source returns `None`, and the caller continues with the next row.
pub fn readiness_first_row(source: Option<DataSource>) -> Option<PlanReadiness> {
    match DataSource::or_sample(source) {
        DataSource::Real => None,
        DataSource::Sample => Some(PlanReadiness::SamplePlan),
    }
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
        ];
        for (source, expected) in rows {
            assert_eq!(readiness_first_row(source), expected, "{source:?}");
        }
    }
}
