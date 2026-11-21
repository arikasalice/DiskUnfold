//! F28 change submission and F11 current-session item-result contracts.
//! Design items: F28 FR10.HD1.app-host.DD1; F11 FR1.HD1.app-host.DD1,
//! FR10.HD1.app-host.DD1.

use crate::{
    Id, approval_ipc::ApprovalState, history_ipc::HistoryItemRecording, job::JobState,
    values::ByteSize,
};
use serde::{Deserialize, Serialize};

/// F28 approved-change request command (FR8.HD1.app-host.DD1).
pub const CHANGE_EXECUTE: &str = "change.execute";
/// F11 current-session result-page command (FR10.HD1.app-host.DD1).
pub const CHANGE_ITEM_RESULTS: &str = "change.item_results";

/// Stable F11 item reason keys (FR3.HD1.app-host.DD1, FR3.HD1.app-host.DD2).
pub const CHANGE_ITEM_IDENTITY_UNSUPPORTED: &str = "change.item.identity_unsupported";
/// Stable F11 item reason keys (FR3.HD1.app-host.DD1).
pub const CHANGE_ITEM_CHANGED: &str = "change.item.changed";
/// Stable F11 item reason keys (FR6.HD1.app-host.DD2).
pub const CHANGE_ITEM_IDENTITY_CHANGED: &str = "change.item.identity_changed";
/// Stable F11 item reason keys (FR4.HD1.app-host.DD1).
pub const CHANGE_ITEM_CHILDREN_CHANGED: &str = "change.item.children_changed";
/// Stable F11 item reason keys (FR12.HD1.app-host.DD1).
pub const CHANGE_ITEM_KEEP_COPY_CHANGED: &str = "change.item.keep_copy_changed";
/// Stable F11 item reason keys (FR13.HD1.app-host.DD1).
pub const CHANGE_ITEM_PLAN_CONFLICT: &str = "change.item.plan_conflict";
/// Stable F11 item reason keys (FR7.HD1.app-host.DD1).
pub const CHANGE_ITEM_PERMISSION_DENIED: &str = "change.item.permission_denied";
/// Stable F11 item reason keys (FR2.HD1.app-host.DD1).
pub const CHANGE_ITEM_TRASH_UNSUPPORTED: &str = "change.item.trash_unsupported";
/// Stable F11 item reason keys (FR14.HD1.app-host.DD2).
pub const CHANGE_ITEM_IDENTITY_RACE: &str = "change.item.identity_race";
/// Stable F11 item reason keys (FR7.HD1.app-host.DD1).
pub const CHANGE_ITEM_FILE_IN_USE: &str = "change.item.file_in_use";
/// Stable F11 item reason keys (FR7.HD1.app-host.DD1).
pub const CHANGE_ITEM_OS_FAILED: &str = "change.item.os_failed";
/// Stable F11 item reason keys (FR7.HD1.app-host.DD1).
pub const CHANGE_ITEM_VOLUME_UNAVAILABLE: &str = "change.item.volume_unavailable";
/// Stable F11 item reason keys (FR11.HD1.app-host.DD1).
pub const CHANGE_ITEM_OUTCOME_UNKNOWN: &str = "change.item.outcome_unknown";
/// Stable F11 item reason keys (FR8.HD1.app-host.DD2).
pub const CHANGE_ITEM_CANCELLED: &str = "change.item.cancelled";

/// Terminal item states returned by F11 (FR6.HD1.app-host.DD1, FR7.HD1.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ChangeItemState {
    Completed,
    Skipped,
    Failed,
    Unprocessed,
}

/// Optional display-result filter for the six F11 result groups (FR10.HD1.app-host.DD1).
/// `Failed` and `OutcomeUnknown` both select failed items; `Held` and `Skipped`
/// both select skipped items, distinguished by their corresponding flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ChangeItemResultFilter {
    Completed,
    Failed,
    OutcomeUnknown,
    Skipped,
    Held,
    Unprocessed,
}

/// Starts an already-approved immutable request version (F28 FR10.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeExecuteRequest {
    pub request_version_id: Id,
}

/// Immediate result of registering the approved change job (F28 FR10.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChangeExecuteResponse {
    pub job_ref: Id,
    pub approval_state: ApprovalState,
}

/// Reads the current-session page for a change job (F11 FR10.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeItemResultsRequest {
    pub job_ref: Id,
    /// Omitted or null means all recorded result groups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub result_filter: Option<ChangeItemResultFilter>,
    /// Omitted or null starts at the first result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub cursor: Option<String>,
}

/// Counts for one current-session change job (F11 FR7.HD1.app-host.DD2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChangeItemCounts {
    pub completed: u32,
    /// The shared job summary includes this category; `change.trash` leaves it zero.
    pub overwritten: u32,
    pub skipped: u32,
    pub failed: u32,
    pub unprocessed: u32,
}

/// One item result, with a session-only undo availability marker (F11 FR6.HD1.app-host.DD1, FR10.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChangeItemResult {
    pub item_ref: crate::ItemRef,
    /// F11 `change.trash` uses `state`; it does not emit an `overwritten` item state.
    pub state: ChangeItemState,
    /// Omitted or null means no item-specific reason was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reason_key: Option<String>,
    /// Omitted or null means the size was not observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observed_size: Option<ByteSize>,
    pub review_needed: bool,
    pub held: bool,
    pub outcome_unknown: bool,
    pub undo_available: bool,
}

/// Stable page and job summary. `item_recording` reuses history contracts (F11 FR10.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChangeItemResultsResponse {
    pub job_ref: Id,
    pub job_state: JobState,
    pub counts: ChangeItemCounts,
    pub item_recording: HistoryItemRecording,
    pub items: Vec<ChangeItemResult>,
    /// Omitted or null means there is no next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests;
