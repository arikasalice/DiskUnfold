//! F03 item-space observations from one pinned analysis result.
use crate::{Id, ItemRef, SizeValue, Timestamp, browse_ipc::BrowseValue};
use serde::{Deserialize, Serialize};

/// Requests space evidence for an item in one pinned result version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceDetailRequest {
    pub result_version: Id,
    pub item: ItemRef,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpaceDetailRequestFields {
    result_version: Id,
    #[serde(deserialize_with = "crate::analysis_ipc::deserialize_f03_request_item_ref")]
    item: ItemRef,
}

impl<'de> Deserialize<'de> for SpaceDetailRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields: SpaceDetailRequestFields =
            crate::analysis_ipc::deserialize_f03_request_object(deserializer)?;
        Ok(Self {
            result_version: fields.result_version,
            item: fields.item,
        })
    }
}

/// Preserves the original size and the limits on its measurement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceDetailSize {
    pub size: SizeValue,
    pub measurement_limits: Vec<String>,
}

/// Paths sharing a verified file identity in the result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceLinkGroup {
    /// Total OS-reported links, serialized as an exact decimal count.
    pub reported_link_count: Option<String>,
    pub in_scope_paths: Vec<ItemRef>,
    pub has_outside_links: Option<bool>,
}

/// First matching exclusive-space decision, independently of the size values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceExclusiveDecision {
    pub confirmed: bool,
    pub reason_key: Option<String>,
}

/// Reported local residency of a confirmed placeholder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SpacePlaceholderState {
    None,
    Partial,
    Full,
}

/// Auxiliary allocation and whether the observed allocation includes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceAuxiliary {
    pub size: SizeValue,
    pub included_in_allocated: Option<bool>,
}

/// Support status reported with the stored observation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SpaceCapabilityState {
    Supported,
    Partial,
    Unsupported,
}

/// An observed capability, with an open key for new filesystem evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceDetailCapability {
    pub key: String,
    pub state: SpaceCapabilityState,
    pub reason_key: Option<String>,
}

/// Space values and their provenance; absent or unknown values carry no bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceDetail {
    pub item: ItemRef,
    pub logical: BrowseValue<SpaceDetailSize>,
    pub allocated: BrowseValue<SpaceDetailSize>,
    pub unique_physical: BrowseValue<SpaceDetailSize>,
    pub exclusive: BrowseValue<SpaceDetailSize>,
    pub shared: BrowseValue<SpaceDetailSize>,
    pub link_group: BrowseValue<SpaceLinkGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub placeholder_local_state: Option<BrowseValue<SpacePlaceholderState>>,
    pub auxiliary: BrowseValue<SpaceAuxiliary>,
    pub exclusive_decision: SpaceExclusiveDecision,
    pub observed_at: BrowseValue<Timestamp>,
    pub capabilities: BrowseValue<Vec<SpaceDetailCapability>>,
}
