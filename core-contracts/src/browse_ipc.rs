//! Main-window browse detail contract for immutable observations.
use crate::{
    Id, ItemRef, SizeValue, Timestamp,
    result::{ResultCompleteness, ResultItemKind},
};
use serde::Deserialize;

pub use crate::browse_value::BrowseValue;

/// Copies paths observed in one pinned result revision.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowseCopyPathRequest {
    pub result_version: Id,
    #[serde(deserialize_with = "deserialize_browse_request_item_refs")]
    pub items: Vec<ItemRef>,
}

/// The local copy outcome. Clipboard contents are private to the desktop host.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "outcome",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BrowseCopyPathOutcome {
    Copied { count: u32 },
    Refused { reason_key: BrowseCopyPathRefusal },
}

/// Stable reason keys for a refused clipboard operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum BrowseCopyPathRefusal {
    #[serde(rename = "path.unrepresentable")]
    Unrepresentable,
    #[serde(rename = "path.multiline")]
    Multiline,
    #[serde(rename = "clipboard.unsupported")]
    Unsupported,
    #[serde(rename = "clipboard.failed")]
    Failed,
}

/// Identifies an item only within the result version named by this request.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowseDetailRequest {
    pub result_version: Id,
    #[serde(deserialize_with = "deserialize_browse_request_item_ref")]
    pub item: ItemRef,
}

/// Uses the existing request-only ItemRef validator without tightening the shared value type.
fn deserialize_browse_request_item_ref<'de, D>(deserializer: D) -> Result<ItemRef, D::Error>
where
    D: serde::Deserializer<'de>,
{
    crate::analysis_ipc::deserialize_f03_request_item_ref(deserializer)
}

#[derive(Deserialize)]
#[serde(transparent)]
struct BrowseRequestItemRef(
    #[serde(deserialize_with = "deserialize_browse_request_item_ref")] ItemRef,
);

/// Applies request-only ItemRef validation to each member of the copy-path array.
fn deserialize_browse_request_item_refs<'de, D>(deserializer: D) -> Result<Vec<ItemRef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<BrowseRequestItemRef>::deserialize(deserializer)
        .map(|items| items.into_iter().map(|item| item.0).collect())
}

/// Result origin supplied by the current analysis result store.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum BrowseOrigin {
    Scan,
}

/// Observed attributes for display. The native path bytes never cross this interface.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BrowseDetail {
    pub item: ItemRef,
    /// Human-readable display text; never reopen or compare a path from it.
    pub display_path: BrowseValue<String>,
    /// Human-readable display text; never reopen or compare a path from it.
    pub display_name: BrowseValue<String>,
    pub kind: BrowseValue<ResultItemKind>,
    pub logical_size: BrowseValue<SizeValue>,
    pub allocated_size: BrowseValue<SizeValue>,
    pub modified_at: BrowseValue<Timestamp>,
    pub created_at: BrowseValue<Timestamp>,
    pub observed_at: BrowseValue<Timestamp>,
    pub origin: BrowseOrigin,
    /// Data source of the pinned version; absent means a sample (F32 FR2.HD2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub data_source: Option<crate::data_source::DataSource>,
    pub completeness: ResultCompleteness,
    pub current_correspondence: bool,
    pub observation_only: bool,
    /// The scan state of a folder, the same value its row carries in a folder page
    /// (F02 #347): whether it or a place inside it was not read, so a lower bound of 0
    /// can be told from a lower bound where every counted item is empty. Omitted for a
    /// file and for a sample. `null` is rejected.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::folder_flags::non_null"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub folder_flags: Option<crate::folder_flags::FolderFlags>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const ID: &str = "018f0000-0000-7000-8000-000000000001";

    fn with_field(mut value: Value, pointer: &str) -> Result<Value, String> {
        value
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| pointer.to_owned())?
            .insert("futureField".into(), json!(true));
        Ok(value)
    }

    #[test]
    fn detail_and_copy_path_requests_reject_unknown_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        let detail = json!({"resultVersion":ID,"item":{"resultVersion":ID,"itemNo":0}});
        serde_json::from_value::<BrowseDetailRequest>(detail.clone())?;
        for pointer in ["", "/item"] {
            let value = with_field(detail.clone(), pointer)?;
            assert!(serde_json::from_value::<BrowseDetailRequest>(value).is_err());
        }
        let copy = json!({"resultVersion":ID,"items":[{"resultVersion":ID,"itemNo":0}]});
        serde_json::from_value::<BrowseCopyPathRequest>(copy.clone())?;
        for pointer in ["", "/items/0"] {
            let value = with_field(copy.clone(), pointer)?;
            assert!(serde_json::from_value::<BrowseCopyPathRequest>(value).is_err());
        }
        Ok(())
    }
}
