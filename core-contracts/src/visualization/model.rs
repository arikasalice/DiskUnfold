//! Model-local references shared by visualization strategies and consumers.

use super::geometry::Geometry;
use crate::ItemRef;
use serde::{Deserialize, Serialize};

/// Hierarchy fields supplied together by new treemap models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ShapeMeta {
    /// Opaque, unique identifier scoped to this model.
    pub shape_ref: String,
    /// Closest returned ancestor, or null when none was returned.
    pub parent_shape_ref: Option<String>,
    /// Depth from the navigation root, including any omitted ancestors.
    pub visible_depth: u32,
    /// First folder in the branch, from the same result revision.
    pub root_branch_ref: Option<ItemRef>,
}

/// An open relationship kind between shapes within one model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizRelationship {
    /// Registered key preserved even when the consumer has no matching style.
    pub kind: String,
    pub from_shape_ref: String,
    pub to_shape_ref: String,
    /// Drawing information, when the strategy supplies geometry for the relationship.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub geometry: Option<Geometry>,
}
