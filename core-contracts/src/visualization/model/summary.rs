//! The summary fields of the items a treemap does not draw (F06 README 51.3).

use super::VizSummary;
use crate::visualization::quantity::VizAmount;

impl VizSummary {
    /// The size and first row of the items not drawn come only with such items; the
    /// size is in bytes and never an exact zero. The row bound is the producer's: the
    /// model does not hold the table.
    pub(super) fn validate(&self) -> Result<(), &'static str> {
        let none_unknown =
            matches!(self.unknown_items.amount(), VizAmount::Exact { value } if value.get() == 0);
        if none_unknown && (self.unknown_size.is_some() || self.first_unknown_row.is_some()) {
            return Err("unknown size or row without unknown items");
        }
        if let Some(size) = &self.unknown_size
            && (size.unit() != "bytes"
                || matches!(size.amount(), VizAmount::Exact { value } if value.get() == 0))
        {
            return Err("invalid unknown size");
        }
        Ok(())
    }
}
