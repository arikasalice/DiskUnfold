//! Resource controls for metadata collection (F02 FR7).
use crate::Id;
use serde::{Deserialize, Serialize};

/// Requested scan resource strategy. Unknown values are rejected at IPC ingress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ScanResourceMode {
    Auto,
    Quiet,
    Fast,
}

/// Optional controls supplied when a scan starts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanCollectionCondition {
    /// Omission selects `auto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub resource_mode: Option<ScanResourceMode>,
    /// Omission leaves the strategy's worker limit unrestricted by the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub user_limit: Option<u32>,
}

/// The effective resource condition returned for a scan job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanCollection {
    pub resource_mode: ScanResourceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub user_limit: Option<u32>,
}

/// Open strategy identity attached to lane status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanLaneStrategy {
    /// Receivers treat an unknown ID as an unknown strategy.
    pub id: String,
    pub version: u32,
}

/// Full replacement of the live resource condition for an owned scan job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanSetResourceRequest {
    pub job_ref: Id,
    pub resource_mode: ScanResourceMode,
    /// Omission clears the prior user limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub user_limit: Option<u32>,
}

/// Result of applying a live resource condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ScanSetResourceOutcome {
    Accepted,
    NoEffect,
    AlreadyTerminal,
}

/// Effective worker limit reported for one lane after a resource update.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanSetResourceLane {
    pub lane_index: u32,
    pub effective_limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub permit: Option<u32>,
}

/// Acknowledgement of a live or queued resource update.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanSetResourceResponse {
    pub outcome: ScanSetResourceOutcome,
    pub lanes: Vec<ScanSetResourceLane>,
    /// An unknown value is interpreted as `next_assignment` by receivers.
    pub applies_from: String,
}
