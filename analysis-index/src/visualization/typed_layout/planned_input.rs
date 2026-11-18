//! Layout input built by the level-of-detail policy instead of admitted from siblings.
use super::{Input, NumericRow};
use crate::visualization::{
    geometry::{Rect, valid_bounds},
    squarify, treemap,
};
use du_core_contracts::{ItemRef, SizeValue};

impl NumericRow {
    /// A row of a planned level. A group or blank row names a representative item and
    /// carries a policy-computed weight; no strategy reads it as an observed item.
    pub(crate) fn planned(item: ItemRef, value: SizeValue, bytes: u64) -> Self {
        Self { item, value, bytes }
    }
}

impl Input {
    /// Wraps rows already ordered by non-increasing weight, without sibling admission.
    pub(crate) fn planned(
        rows: Vec<NumericRow>,
        bounds: Rect,
    ) -> Result<Self, treemap::AdmissionError> {
        if !valid_bounds(bounds) {
            return Err(treemap::AdmissionError::Layout(
                squarify::LayoutError::InvalidBounds,
            ));
        }
        Ok(Self { rows, bounds })
    }
}
