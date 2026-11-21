//! The model summary: grouped, omitted and unknown counts, and the items a treemap does
//! not draw (F06 README 51.3) or could not read (README 48).

use super::super::{optional_present, quantity::VizQuantity};
use crate::visualization::quantity::VizAmount;
use serde::{Deserialize, Serialize};

/// Explicit counts for grouped, omitted and unknown items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizSummary {
    pub grouped_items: VizQuantity,
    pub omitted_items: VizQuantity,
    pub unknown_items: VizQuantity,
    pub reason_keys: Vec<String>,
    /// Bytes of the scope measured under navigation-root items without a provable area
    /// and outside every drawn item (F06 README 51.3). Treemap only; omitted when no
    /// such item holds a measured byte, and never an exact zero.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_present"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub unknown_size: Option<VizQuantity>,
    /// Position in the navigation root's table (viz.rows without parentShapeRef) of the
    /// first row without a provable area, or else of the first expanded folder whose
    /// level has one. Treemap only; omitted when no table row is such a row.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_present"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub first_unknown_row: Option<u32>,
    /// Folders below the model's scope whose listing failed (F06 README 48, #346): a
    /// recorded scan gap, or a folder a cancellation or disconnection left unread. Exact
    /// from the recorded gaps; a lower bound while the scan runs or when such marks make
    /// the count incomplete. Omitted when there are none, never an exact zero. These
    /// folders are not in `unknownItems`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_present"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub unread_folders: Option<VizQuantity>,
    /// Position in the navigation root's table of the first row that is such a folder
    /// or holds one. Omitted with `unreadFolders`, or when no table row holds one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_present"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub first_unread_row: Option<u32>,
}

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
        if let Some(folders) = &self.unread_folders
            && (folders.unit() != "items"
                || matches!(folders.amount(), VizAmount::Exact { value } if value.get() == 0))
        {
            return Err("invalid unread folders");
        }
        if self.first_unread_row.is_some() && self.unread_folders.is_none() {
            return Err("unread row without unread folders");
        }
        Ok(())
    }
}
