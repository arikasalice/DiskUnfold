//! Selection-independent item location in a pinned F06 model.

use crate::ItemRef;
use serde::{Deserialize, Deserializer, Serialize, de::Error};

/// Looks up an observed item in an existing model without changing selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VizLocateRequest {
    /// Opaque reference returned by rendering the model.
    #[serde(deserialize_with = "nonempty_reference")]
    pub model_ref: String,
    /// Item to locate, bound to its original result revision.
    pub item_ref: ItemRef,
}

/// The item's location in the model's shape array, group, or scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "location",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum VizLocate {
    /// Zero-based index into the shape array of the requested model.
    Shape { shape_index: u32 },
    /// The containing group, identified within the requested model.
    Group {
        #[serde(deserialize_with = "nonempty_reference")]
        group_ref: String,
    },
    /// The item is in the model's scope and has no shape in this view.
    OutsideView {},
    /// The item exists in the same result outside the model's scope.
    OutsideScope {},
}

fn nonempty_reference<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let reference = String::deserialize(deserializer)?;
    if reference.is_empty() {
        return Err(D::Error::custom("empty visualization reference"));
    }
    Ok(reference)
}
