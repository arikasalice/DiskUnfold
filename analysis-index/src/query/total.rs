//! F03 totals and observation evidence for pinned query membership.
use super::SpaceTotalError;

/// An F03 query total and observation bounds for its known matched files.
pub struct TotalWithObservationRange<'a> {
    /// The total calculated by the rule fixed in the pinned result.
    pub aggregate: du_core_contracts::result::SpaceAggregate,
    /// Bounds among matched files with timestamps. Incomplete membership, source
    /// coverage, non-file rows or missing timestamps make the range incomplete.
    /// An empty complete set has no bounds and a complete range.
    pub observation_range: crate::ObservationRange<'a>,
}

impl super::QueryResult {
    /// Calculates the pinned set total and its observation bounds in one traversal.
    ///
    /// Cancellation returns no total or range. Bounds are from known matched files,
    /// not from a publication time or inferred directory descendants.
    ///
    /// # Errors
    /// Returns cancellation or an error for an invalid or differently pinned item.
    pub fn space_total_with_observation_range(
        &self,
        cancelled: impl FnMut() -> bool,
    ) -> Result<TotalWithObservationRange<'_>, SpaceTotalError> {
        let (aggregate, range) = self.version.size_rule.selection_total_with_range(
            &self.version,
            &self.items,
            self.unknown_count == 0 && !self.inherited_uncertainty,
            cancelled,
        )?;
        Ok(TotalWithObservationRange {
            aggregate,
            observation_range: range.resolve(self.version.item_observed_times.as_deref()),
        })
    }
}
