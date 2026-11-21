//! F28 approval review, grant, state event, and reason-key contracts.
//! Design items: FR1.HD1.app-host.DD1, FR2.HD1.app-host.DD1,
//! FR10.HD1.app-host.DD1, AFR23.core-contracts.ADD1,
//! AFR31.core-contracts.ADD1.

use crate::{
    Id, ItemRef, Timestamp,
    job::ChangeAction,
    plan_ipc::{PlanHash, PlanItemDisposition},
    values::{ByteSize, OsPath},
};
use serde::{Deserialize, Serialize};

pub use crate::sample_ipc::APPROVAL_SAMPLE_SOURCE_DENIED;

/// F28 GUI command for reading a request-version review (FR1.HD1.app-host.DD1).
pub const APPROVAL_REVIEW: &str = "approval.review";
/// F28 GUI command for granting approval for a reviewed request (FR2.HD1.app-host.DD1).
pub const APPROVAL_GRANT: &str = "approval.grant";
/// F28 event topic for request-version approval state (AFR23.core-contracts.ADD1).
pub const APPROVAL_STATE_CHANGED: &str = "approval.state_changed";

/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_NOT_ISSUED: &str = "approval.not_issued";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_ALREADY_USED: &str = "approval.already_used";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_INVALIDATED: &str = "approval.invalidated";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_MISMATCH: &str = "approval.mismatch";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_CHECK_FAILED: &str = "approval.check_failed";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_REQUEST_NOT_FOUND: &str = "approval.request_not_found";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_REQUEST_SUPERSEDED: &str = "approval.request_superseded";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_STALE: &str = "approval.stale";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_PROTECTION_DENIED: &str = "approval.protection_denied";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_PROTECTION_UNDETERMINED: &str = "approval.protection_undetermined";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_CONFIRMATION_MISSING: &str = "approval.confirmation_missing";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_REVIEW_UNAVAILABLE: &str = "approval.review_unavailable";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_PLAN_SUPERSEDED: &str = "approval.plan_superseded";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_RULESET_CHANGED: &str = "approval.ruleset_changed";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_RESTORE_INFO_CLEARED: &str = "approval.restore_info_cleared";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_MOVE_UNDO_INFO_CLEARED: &str = "approval.move_undo_info_cleared";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_USER_REVOKED: &str = "approval.user_revoked";
/// Stable F28 approval reason key for a withdrawn review (AFR31.core-contracts.ADD1).
pub const APPROVAL_REVIEW_WITHDRAWN: &str = "approval.review_withdrawn";
/// Stable F28 approval reason key (AFR31.core-contracts.ADD1).
pub const APPROVAL_IN_USE: &str = "approval.in_use";

/// Kind of immutable request version being reviewed (F28 FR1.HD1.app-host.DD1; Architecture DLD §2.6.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    Plan,
    Restore,
    MoveUndo,
}

/// Additional acknowledgements required for release or overwrite (Architecture DLD §2.6.8; F28 FR5.HD1.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationKind {
    ReleaseDefaultProtection,
    Overwrite,
}

/// Approval registry state returned by review and grant commands (F28 FR2.HD1.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    InReview,
    Granted,
    Rejected,
    Withdrawn,
    InUse,
    Spent,
    Invalidated,
}

/// Coarse state values published by `approval.state_changed` (F28 AFR23.core-contracts.ADD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ApprovalEventState {
    Granted,
    Consumed,
    Revoked,
    Rejected,
}

/// Per-item protection result included in an approval review (F28 FR1.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalProtectionResult {
    pub item_ref: ItemRef,
    pub disposition: PlanItemDisposition,
    /// Omitted or null means no protection reason was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reason_key: Option<String>,
}

/// Basic-protection facts shown with the review summary (F28 FR1.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalProtectionSummary {
    pub ruleset_version: String,
    pub class_table_version: String,
    pub unlocked_classes: Vec<String>,
    pub unlocked_item_count: u32,
    pub unlocked_bytes: ByteSize,
    pub required_basic_protection_ack: bool,
}

/// Overwrite facts shown with the review summary (F28 FR1.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalOverwriteSummary {
    pub overwrite_item_count: u32,
    pub displaced_bytes: ByteSize,
    pub required_overwrite_ack: bool,
}

/// Review summary pinned to one request version (F28 FR1.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalReviewSummary {
    pub target_count: u32,
    pub target_size: ByteSize,
    pub visible_selection_count: u32,
    pub hidden_selection_count: u32,
    pub hidden_descendant_count: u32,
    pub actions: Vec<ChangeAction>,
    /// Omitted or null means the request has no destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub destination: Option<OsPath>,
    pub retention_conditions: Vec<Id>,
    /// Nested fields are owned by the corresponding feature summaries.
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    pub effect_summary: serde_json::Value,
    pub protection_results: Vec<ApprovalProtectionResult>,
    /// Nested fields are owned by the corresponding feature summaries.
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    pub restoration_limit_summary: serde_json::Value,
    /// Omitted or null means no basic-protection release applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub protection_summary: Option<ApprovalProtectionSummary>,
    /// Omitted or null means no overwrite confirmation applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub overwrite_summary: Option<ApprovalOverwriteSummary>,
}

/// Reads the review summary for one immutable request version (F28 FR1.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalReviewRequest {
    pub request_version_id: Id,
    pub request_kind: RequestKind,
}

/// Review result fixed to the requested version and plan hash (F28 FR1.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalReviewResponse {
    pub request_version_id: Id,
    pub summary: ApprovalReviewSummary,
    pub plan_hash: PlanHash,
    pub approval_possible: bool,
    /// Omitted or null means there are no known blocking reasons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub unavailable_reason_keys: Option<Vec<String>>,
    /// Omitted or null means the registry has no current state for this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub current_approval_state: Option<ApprovalState>,
}

/// The explicit proofs collected by the approval screen (F28 FR2.HD1.app-host.DD1, FR5.HD1.gui-web.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalConfirmationProofs {
    /// Omitted or null means false; required only for permanent-delete requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub permanent_delete_selected: Option<bool>,
    /// Omitted or null means the target count was not confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub confirmed_target_count: Option<u32>,
    /// Omitted or null means the target size was not confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub confirmed_target_size: Option<ByteSize>,
    /// Omitted or null means false; true is required only for permanent-delete requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub irreversibility_confirmed: Option<bool>,
    /// Omitted or null means false; true is required when basic protection is released.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub basic_protection_unlock_confirmed: Option<bool>,
    /// Omitted or null means false; true is required when overwrite is selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub overwrite_confirmed: Option<bool>,
    /// Omitted or null means no overwrite count was confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub confirmed_overwrite_item_count: Option<u32>,
    /// Omitted or null means no displaced size was confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub confirmed_displaced_bytes: Option<ByteSize>,
}

/// Requests approval for the reviewed request version and hash (F28 FR2.HD1.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalGrantRequest {
    pub request_version_id: Id,
    pub presented_plan_hash: PlanHash,
    /// Omitted or null means no additional proof was submitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub confirmations: Option<ApprovalConfirmationProofs>,
}

/// Approval status returned after a grant attempt succeeds (F28 FR2.HD1.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalGrantResponse {
    pub state: ApprovalState,
}

/// Published approval state for a request version; it contains no approval ID (F28 FR4.HD1.app-host.DD1, AFR23.core-contracts.ADD1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalStateChanged {
    pub request_version_id: Id,
    pub state: ApprovalEventState,
    /// Omitted or null means no state-change reason is attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reason_key: Option<String>,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    pub occurred_at: Timestamp,
}

#[cfg(test)]
mod tests;
