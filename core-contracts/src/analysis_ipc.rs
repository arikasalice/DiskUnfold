//! Command and event bodies for selecting, scanning, and reading an analysis scope.
use crate::{ByteSize, Id, ItemRef, OsPath, SizeValue, Timestamp};
use serde::{Deserialize, Serialize};

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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisTarget {
    pub kind: TargetKind,
    pub path: OsPath,
    pub connected: bool,
    pub volume_id: Option<String>,
    pub identity_source: IdentitySource,
    pub access: TargetAccess,
    pub reason_key: Option<String>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum ScopeExclusion {
    Path { path: OsPath },
    NamePattern { pattern: String },
}

/// Scope selected by the caller. An OS adapter verifies the raw paths before use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeDraft {
    pub includes: Vec<OsPath>,
    pub exclusions: Vec<ScopeExclusion>,
    pub boundary: ScopeBoundary,
    pub included_mounts: Vec<OsPath>,
    pub source: ScopeSource,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanStartRequest {
    pub draft: ScopeDraft,
    pub review_fingerprint: String,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalysisJobRequest {
    pub job_ref: Id,
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
    pub reason_key: Option<String>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind",
    deny_unknown_fields
)]
pub enum SpaceTarget {
    ScopeRoot {},
    Folder { item: ItemRef },
    QueryResult { query_ref: Id },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpaceTotalRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub result_version: Option<Id>,
    pub target: SpaceTarget,
    pub request_seq: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceTotalSize {
    pub size: SizeValue,
    pub completeness: crate::result::ResultCompleteness,
    pub measurement_limits: Vec<String>,
}

/// A response bound to a single immutable result version and request sequence.
/// Unavailable counts are null. The missing item count covers known observations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SpaceTotal {
    pub result_version: Id,
    pub request_seq: u32,
    pub sizes: Vec<SpaceTotalSize>,
    pub completeness: crate::result::ResultCompleteness,
    pub missing_items: Option<ByteSize>,
    pub inaccessible_ranges: Option<ByteSize>,
    pub untraversed_links: Option<ByteSize>,
}
