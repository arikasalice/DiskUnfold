//! The type classes of a whole result, for the result summary card (F02 #300).

use crate::{
    Id, SizeBasis,
    visualization::{quantity::VizAmount, type_slices::VizTypeSlice},
};
use serde::{Deserialize, Serialize};

/// Asks for the legend classes of the pinned result in one size basis.
///
/// GUI only. The classes are counted as the legend of the space distribution counts
/// them, with one set of counted files for every scope root, so the two never
/// disagree for the same result and basis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpaceTypeSummaryRequest {
    pub result_version: Id,
    pub basis: SizeBasis,
}

/// The classes of a result's files, largest first.
///
/// `slices` holds one entry per class with at least one counted file: bytes
/// descending, ties in legend order, at most [`MAX_TYPE_SLICES`]. An empty list means
/// every range was read and there is no file; it is never used for a result that could
/// not be counted (see `lower_bound`). Every `class_key` of `slices` is in
/// `legend_order`, which lists the classes of the legend in legend order, so a screen
/// gives a class the same colour and pattern as the legend of the chart.
///
/// The slices never add up to more than the result's size; what the size holds beyond
/// them (folders' own allocation, unread or uncounted content) is not a slice, and when
/// a slice or the size is a lower bound that difference is unknown, never exact.
///
/// [`MAX_TYPE_SLICES`]: crate::visualization::type_slices::MAX_TYPE_SLICES
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceTypeSummary {
    pub result_version: Id,
    /// The basis of the request, returned so that a reader pairs an answer with the
    /// basis it asked for and never draws a value of one basis beside another.
    pub basis: SizeBasis,
    pub slices: Vec<VizTypeSlice>,
    pub legend_order: Vec<String>,
    /// A range of the result was not read, or whether one was cannot be told: the
    /// slices, and an empty list, are then only lower bounds.
    pub lower_bound: bool,
}

impl SpaceTypeSummary {
    /// Checks the response: valid slices, every slice class in the legend order, and
    /// `lower_bound` set whenever a slice is a lower bound (one direction: a result with
    /// an unread range may still have only exact slices).
    ///
    /// # Errors
    /// A message naming the broken rule.
    pub fn validate(&self) -> Result<(), &'static str> {
        VizTypeSlice::validate_list(&self.slices)?;
        if self.legend_order.len() > crate::visualization::type_slices::MAX_TYPE_SLICES {
            return Err("too many legend classes");
        }
        if self
            .slices
            .iter()
            .any(|slice| !self.legend_order.contains(&slice.class_key))
        {
            return Err("a slice class is not in the legend order");
        }
        let lower = |amount: &VizAmount| matches!(amount, VizAmount::LowerBound { .. });
        if !self.lower_bound
            && self
                .slices
                .iter()
                .any(|slice| lower(slice.item_count.amount()) || lower(slice.total.amount()))
        {
            return Err("a slice is a lower bound but the answer is not");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visualization::quantity::VizQuantity;

    fn summary(total: VizQuantity, lower_bound: bool) -> Result<SpaceTypeSummary, String> {
        let exact_items = VizQuantity::exact("items", 3).ok_or("items")?;
        Ok(SpaceTypeSummary {
            result_version: Id::from_v7_bytes([1; 16]),
            basis: SizeBasis::Allocated,
            slices: vec![VizTypeSlice {
                class_key: "type.video".into(),
                item_count: exact_items,
                total,
            }],
            legend_order: vec!["type.video".into()],
            lower_bound,
        })
    }

    #[test]
    fn a_lower_bound_slice_needs_a_lower_bound_answer_but_not_the_other_way() -> Result<(), String>
    {
        let lower = VizQuantity::lower_bound("bytes", 10).ok_or("bytes")?;
        let exact = VizQuantity::exact("bytes", 10).ok_or("bytes")?;
        assert!(summary(lower.clone(), true)?.validate().is_ok());
        assert!(summary(lower, false)?.validate().is_err());
        // An unread range elsewhere may leave every counted slice exact.
        assert!(summary(exact.clone(), true)?.validate().is_ok());
        assert!(summary(exact, false)?.validate().is_ok());
        Ok(())
    }
}
