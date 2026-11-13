//! F08 adapter to F03 aggregation of pinned, established file membership.
use super::summary::{SetTotals, Side};
use du_analysis_index::{ResultVersion, query::SpaceTotalError};
use du_core_contracts::{ItemRef, SizeBasis, result::SpaceAggregate};
use std::sync::Arc;

/// One pinned result and the established common-range file membership for it.
///
/// The caller must resolve item references against this version, apply the
/// same common range on both sides, and mark membership incomplete whenever
/// coverage or correspondence cannot be established. Snapshot record numbers
/// are not assumed to be item numbers in the loaded result version.
pub struct Selection {
    version: Arc<ResultVersion>,
    items: Vec<ItemRef>,
    complete: bool,
}

impl Selection {
    /// Keeps the pinned version and membership alive for the comparison.
    #[must_use]
    pub fn new(version: Arc<ResultVersion>, items: Vec<ItemRef>, complete: bool) -> Self {
        Self {
            version,
            items,
            complete,
        }
    }
}

/// F03 totals over two independently pinned, already resolved selections.
pub struct Totals {
    earlier: Selection,
    later: Selection,
}

impl Totals {
    /// Binds each side to its own version and common-range membership.
    #[must_use]
    pub fn new(earlier: Selection, later: Selection) -> Self {
        Self { earlier, later }
    }
}

impl SetTotals for Totals {
    type Error = SpaceTotalError;

    fn total(
        &mut self,
        side: Side,
        _: &SizeBasis,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<SpaceAggregate, Self::Error> {
        let selection = match side {
            Side::Earlier => &self.earlier,
            Side::Later => &self.later,
        };
        selection
            .version
            .selection_total(&selection.items, selection.complete, cancelled)
    }
}

#[cfg(test)]
mod tests;
