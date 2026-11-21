//! Command and event bodies for selecting, scanning, and reading an analysis scope.
use crate::{ByteSize, Id, OsPath, Timestamp};
use serde::{Deserialize, Serialize};

mod f03_request;
mod job_progress;
mod lane_status;
mod recent_scope;
mod scan_resource;
mod scope_draft;
mod space_query;

pub use crate::scan_observation::{DeviceKind, DeviceKindSource};
pub(crate) use f03_request::{
    deserialize_f03_request_item_ref, deserialize_f03_request_object,
    deserialize_optional_f03_request_item_ref,
};
pub use job_progress::{AnalysisJobProgress, JOB_PROGRESS};
pub use lane_status::{
    MAX_LANE_ROOTS, MAX_LANES, SCAN_LANE_STATUS, ScanLane, ScanLaneStatus, ScanLaneWait,
};
pub use recent_scope::{
    RecentScopeList, RecentScopeListRequest, RecentScopeListRow, RecentScopeOpen,
    RecentScopeOpenRequest, RecentScopeOpenTarget, RecentScopeTargetState,
};
pub use scan_resource::{
    ScanCollection, ScanCollectionCondition, ScanLaneStrategy, ScanResourceMode,
    ScanSetResourceLane, ScanSetResourceOutcome, ScanSetResourceRequest, ScanSetResourceResponse,
};
pub use scope_draft::{ScopeDraft, ScopeExclusion};
pub use space_query::{
    SpaceObservationRange, SpaceTarget, SpaceTotal, SpaceTotalRequest, SpaceTotalSize,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Internal,
    External,
    NetworkMount,
    FolderMount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum IdentitySource {
    Confirmed,
    Estimated,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TargetAccess {
    Checking,
    Accessible,
    Denied,
    Disconnected,
    Missing,
    Unresponsive,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetListRequest {
    pub refresh: bool,
}

/// A mount entry identified by its native path, with independent identity evidence.
/// The kind is absent when the OS reports the path without evidence for a kind.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisTarget {
    pub kind: Option<TargetKind>,
    pub path: OsPath,
    pub connected: bool,
    pub volume_id: Option<String>,
    pub identity_source: IdentitySource,
    pub access: TargetAccess,
    pub reason_key: Option<String>,
    /// Whether the target belongs in the default list (F01 #263). The engine always
    /// sets it; absent means listed. It only arranges the list and never limits a scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub default_visible: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TargetList {
    pub targets: Vec<AnalysisTarget>,
    pub probe_job_ref: Option<Id>,
    pub observed_at: Timestamp,
    pub unavailable_reason: Option<String>,
}

/// A completed probe. The event envelope carries the probe job and sequence.
/// The observation time is the time the probe determined its outcome.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TargetStatusChanged {
    pub target: OsPath,
    pub access: TargetAccess,
    pub reason_key: Option<String>,
    pub total_bytes: Option<ByteSize>,
    pub available_bytes: Option<ByteSize>,
    pub capacity_reason: Option<String>,
    pub aggregation_uncertain: bool,
    pub observed_at: Timestamp,
}

/// One observed target, ordered within its probe job.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TargetProbeEvent {
    #[serde(flatten)]
    pub observation: TargetStatusChanged,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ScopeSource {
    Gui,
    Recent,
    Catalog,
    Cli,
    Mcp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ScopeBoundary {
    CurrentVolume,
    IncludeSelected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeReviewRequest {
    pub draft: ScopeDraft,
    pub request_seq: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScopeCorrespondence {
    pub path: OsPath,
    pub access: TargetAccess,
    pub reason_key: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScopeReview {
    pub scope: ScopeDraft,
    pub notices: Vec<String>,
    pub targets: Vec<ScopeCorrespondence>,
    pub review_fingerprint: String,
    pub observed_at: Timestamp,
    pub request_seq: u32,
    /// The macOS root volume group the reviewed scope analyses as one volume (#263).
    /// Omitted when the scope is not a volume group. GUI channel only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub volume_group: Option<ScopeVolumeGroup>,
    /// The user's home folder, offered only with
    /// [`ScopeVolumeGroup::NOTICE_UNVERIFIED`] so the user can pick their own files
    /// in one step (#263). A suggestion, not a permission: choosing it replaces the
    /// draft and the new draft is reviewed as usual. It never changes a fingerprint.
    /// Omitted otherwise; `null` is rejected. GUI channel only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "non_null"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub suggested_include: Option<OsPath>,
}

/// An optional field that, when present, must not be `null`.
fn non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// A root volume group in a reviewed scope (#263).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScopeVolumeGroup {
    /// Mount path of the data volume, counted together with `/`.
    pub data_mount: OsPath,
}

impl ScopeVolumeGroup {
    /// Notice: the system and data volumes are analysed together, each file once.
    pub const NOTICE_MERGED: &'static str = "scope.volume_group_merged";
    /// Notice: the data volume could not be confirmed, only the system volume is
    /// analysed. Neither notice is an error or blocks a selection.
    pub const NOTICE_UNVERIFIED: &'static str = "scope.volume_group_unverified";
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanStartRequest {
    pub draft: ScopeDraft,
    pub review_fingerprint: String,
    /// Resource controls are not part of the reviewed scope fingerprint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub collection: Option<ScanCollectionCondition>,
}

/// Admission receipt; collection proceeds independently of the request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanStart {
    pub job_ref: Id,
    pub existing: bool,
    pub scope_version: Id,
    pub scope_fingerprint: String,
    /// The current resource condition of this job, including on duplicate starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub collection: Option<ScanCollection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub lineage_id: Option<Id>,
}

/// A scan job's lineage and current publication, observed together.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub lineage_id: Id,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub result_version: Option<Id>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalysisJobRequest {
    pub job_ref: Id,
}

/// Optional state filter for a consistent desktop job table read.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalysisJobListRequest {
    /// Omission selects all states; an empty list selects none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub states: Option<Vec<crate::job::JobState>>,
}

/// Scheduler priority reported by the desktop job table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AnalysisJobPriority {
    High,
    Normal,
    Low,
}

/// The currently observed wait reason for a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AnalysisJobWaitReason {
    None,
    Paused,
    QueueFull,
    IoWait,
    DeviceCap,
    Backpressure,
    /// The runtime could not establish a wait reason.
    Unknown,
}

/// One row of the desktop's in-process job table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJobListRow {
    pub job_ref: Id,
    pub kind: String,
    pub scope_summary: String,
    /// Display string of the first scope root captured at submission.
    /// Omitted, together with `scope_root_count`, for a kind without scope roots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub scope_first_root: Option<String>,
    /// Number of scope roots captured at submission (1 or more).
    /// Omitted, together with `scope_first_root`, for a kind without scope roots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub scope_root_count: Option<u32>,
    pub state: crate::job::JobState,
    pub wait_reason: AnalysisJobWaitReason,
    pub priority: AnalysisJobPriority,
    /// Supported controls for the job kind, independent of state eligibility.
    pub supported_controls: Vec<crate::job::JobControl>,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    /// Present only after the terminal record's history outcome is confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub terminal: Option<AnalysisJobTerminal>,
}

/// The observed reason that the desktop event relay discarded a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobResyncReason {
    SubscriberQueueLimit,
    SubscriberDeliveryFailed,
}

/// Describes the dropped events up to the delivery of the resync notice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct JobResyncRequired {
    pub reason: JobResyncReason,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub discarded_events: u64,
    pub observed_at: crate::Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJobStatus {
    pub job_ref: Id,
    pub kind: String,
    pub state: crate::job::JobState,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    pub result_version: Option<Id>,
    /// Published query result for a successfully completed query evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub query_ref: Option<Id>,
    pub reason_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub scan_result: Option<ScanResult>,
    /// Confirmed terminal outcome, omitted while history handling is pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub terminal: Option<AnalysisJobTerminal>,
}

/// The terminal outcome recoverable after a job event is missed. The counts
/// follow [`crate::job::JobCounts`]: each unit is in exactly one of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJobTerminal {
    pub state: crate::job::JobState,
    /// Per-job sequence of the terminal event.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    /// 0 when the job ended without a result: its counted units are then in
    /// `unprocessed`, so `completed + failed` can be below its last progress.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub completed: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub overwritten: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub failed: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub skipped: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub unprocessed: u64,
    /// Whether the terminal summary is present in the history store.
    pub history_recorded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reason_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub unprocessed_summary: Option<String>,
    /// Immutable analysis version, if the job produced one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub result_version: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub query_ref: Option<Id>,
    /// History storage warning, absent when recording succeeded or was skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub warning: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobControlOutcome {
    Accepted,
    NoEffect,
    Unsupported,
    AlreadyTerminal,
}

/// A control acknowledgement. A separate status read reports eventual completion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJobControl {
    pub outcome: JobControlOutcome,
    pub state: crate::job::JobState,
    pub reason_key: Option<String>,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
}
