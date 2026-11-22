//! Sankey node columns, link amounts and node identities (F06 README 12.1; #296).
//!
//! Only a Sankey model carries these. Its shapes are the nodes of three columns, its
//! `viz.sankey.link` relationships the links between them, and each node row lists the
//! links leaving it, so the table reaches every link.

use super::quantity::{VizAmount, VizQuantity};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The relationship kind of a Sankey link.
pub const SANKEY_LINK_KIND: &str = "viz.sankey.link";
/// The group reference of the node for bytes no child carries.
pub const SANKEY_UNATTRIBUTED_REF: &str = "viz.sankey.unattributed";
/// The label key of that node.
pub const SANKEY_UNATTRIBUTED_LABEL_KEY: &str = "ui.visualization.sankey.unattributed";
/// The group reference of a type node is this prefix and the type's legend key.
pub const SANKEY_TYPE_REF_PREFIX: &str = "viz.sankey.type.";
/// Most links one row may list; a producer lists at most one per type.
pub const MAX_ROW_LINKS: usize = 32;

/// The column of a Sankey node: the scope, a folder, or a type. A receiver ignores a
/// column it does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum VizColumn {
    Scope,
    Folder,
    Type,
    #[serde(other)]
    Unknown,
}

impl VizColumn {
    /// Links go one column to the right only: scope to folder, folder to type.
    pub fn links_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Scope, Self::Folder) | (Self::Folder, Self::Type)
        )
    }
}

/// One link leaving a node row: the row it reaches and its exact bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizLink {
    /// The target row's reference within the same model revision: its item or group key.
    pub to_row_ref: String,
    pub quantity: VizQuantity,
}

impl VizLink {
    /// A link amount is exact, non-negative bytes: a lower bound or an unknown amount
    /// is never drawn as a link.
    pub fn exact_bytes(quantity: &VizQuantity) -> bool {
        quantity.unit() == "bytes"
            && matches!(quantity.amount(), VizAmount::Exact { value } if value.get() >= 0)
    }

    /// Checks a row's links: exact amounts, distinct non-empty targets, at most
    /// [`MAX_ROW_LINKS`].
    pub fn validate_list(links: &[Self]) -> Result<(), &'static str> {
        if links.len() > MAX_ROW_LINKS {
            return Err("too many row links");
        }
        let mut targets = HashSet::new();
        for link in links {
            if link.to_row_ref.is_empty() || !targets.insert(link.to_row_ref.as_str()) {
                return Err("empty or repeated link target");
            }
            if !Self::exact_bytes(&link.quantity) {
                return Err("a link amount is not exact bytes");
            }
        }
        Ok(())
    }
}
