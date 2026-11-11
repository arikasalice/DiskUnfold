//! Main-window browse detail contract for immutable observations.
use crate::{
    Id, ItemRef, SizeValue, Timestamp,
    result::{ResultCompleteness, ResultItemKind},
};

/// Copies paths observed in one pinned result revision.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowseCopyPathRequest {
    pub result_version: Id,
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
    pub item: ItemRef,
}

/// Distinguishes an absent value from a value the source could not establish.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BrowseValue<T> {
    Present { value: T },
    Absent,
    Unknown { reason_key: String },
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
    pub display_path: BrowseValue<String>,
    pub display_name: BrowseValue<String>,
    pub kind: BrowseValue<ResultItemKind>,
    pub logical_size: BrowseValue<SizeValue>,
    pub allocated_size: BrowseValue<SizeValue>,
    pub modified_at: BrowseValue<Timestamp>,
    pub created_at: BrowseValue<Timestamp>,
    pub observed_at: BrowseValue<Timestamp>,
    pub origin: BrowseOrigin,
    pub completeness: ResultCompleteness,
    pub current_correspondence: bool,
    pub observation_only: bool,
}
