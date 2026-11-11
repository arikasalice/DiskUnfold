//! Format-independent numeric rows and view inputs for registered layouts.
use super::Input;
use crate::visualization::geometry::{Rect, Scale};
use du_core_contracts::visualization::quantity::VizQuantity;

/// One caller-owned numeric row, identified by its position in the input.
pub struct CommonRow {
    pub quantity: VizQuantity,
}
/// Numeric rows and view conditions, without format-specific policy.
pub struct CommonInput<'a> {
    pub rows: Vec<CommonRow>,
    pub bounds: Rect,
    pub scale: Scale,
    pub(super) legacy: Option<&'a Input>,
}
impl<'a> CommonInput<'a> {
    /// Creates input from admitted owner-produced numeric rows.
    pub fn new(rows: Vec<CommonRow>, bounds: Rect, scale: Scale) -> Self {
        Self {
            rows,
            bounds,
            scale,
            legacy: None,
        }
    }
    pub(super) fn from_legacy(input: &'a Input) -> Self {
        Self {
            rows: Vec::new(),
            bounds: input.bounds(),
            scale: Scale::Automatic,
            legacy: Some(input),
        }
    }
    /// Whether this input is the borrowed legacy view without common numeric rows.
    pub fn is_legacy(&self) -> bool {
        self.legacy.is_some()
    }
    pub(super) fn row_count(&self) -> usize {
        self.legacy
            .map_or(self.rows.len(), |input| input.rows().len())
    }
}
