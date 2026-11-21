//! A folder's files divided by legend class, beside its value (F06 README 54; #298).

use super::quantity::{VizAmount, VizQuantity};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Most slices one folder may carry. A producer sends at most one per class of the
/// model's legend (nine for the type table today); a longer list is invalid.
pub const MAX_TYPE_SLICES: usize = 32;

/// The files of one legend class at any depth below a folder.
///
/// Only the type color criterion fills slices today, so `class_key` is a key of the
/// model's type legend: a listed type or the class of no type. A class the legend
/// does not list is added into the class of no type, so the key is always in the
/// legend. The count is in items and the total in bytes. Both are lower bounds when a
/// folder below was not read; the total is also a lower bound when a member's size
/// is only a lower bound or unknown.
///
/// The slices of a folder never add up to more than its value, comparing lower bounds
/// as lower bounds. What the value holds beyond them (the folder's own allocation,
/// unread or uncounted content) is not a slice: when the value or a slice is a lower
/// bound, that difference is unknown and is never read as exact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizTypeSlice {
    pub class_key: String,
    pub item_count: VizQuantity,
    pub total: VizQuantity,
}

impl VizTypeSlice {
    /// Checks a folder's slices: known item counts and byte totals, no empty or
    /// repeated key, and no more than [`MAX_TYPE_SLICES`]. An empty list is valid:
    /// every file was counted and there is none.
    pub fn validate_list(slices: &[Self]) -> Result<(), &'static str> {
        if slices.len() > MAX_TYPE_SLICES {
            return Err("too many type slices");
        }
        let known = |quantity: &VizQuantity, unit: &str| {
            quantity.unit() == unit && !matches!(quantity.amount(), VizAmount::Unknown { .. })
        };
        let mut keys = HashSet::new();
        for slice in slices {
            if slice.class_key.is_empty() || !keys.insert(slice.class_key.as_str()) {
                return Err("empty or repeated type slice key");
            }
            if !known(&slice.item_count, "items") || !known(&slice.total, "bytes") {
                return Err("type slice is not a known count and size");
            }
        }
        Ok(())
    }
}
