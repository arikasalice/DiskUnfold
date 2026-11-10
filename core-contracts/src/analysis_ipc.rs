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

/// Selects a scope root, one folder, or one already-admitted search result.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SpaceTarget {
    ScopeRoot {},
    Folder { item: ItemRef },
    QueryResult { query_ref: Id },
}

impl<'de> Deserialize<'de> for SpaceTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            ScopeRoot,
            Folder,
            QueryResult,
        }

        fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
            deserializer: D,
        ) -> Result<Option<T>, D::Error> {
            T::deserialize(deserializer).map(Some)
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Selector {
            kind: Kind,
            #[serde(default, deserialize_with = "present")]
            item: Option<ItemRef>,
            #[serde(default, deserialize_with = "present")]
            query_ref: Option<Id>,
        }

        struct SelectorObject;
        impl<'de> serde::de::Visitor<'de> for SelectorObject {
            type Value = Selector;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a space target object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                Selector::deserialize(serde::de::value::MapAccessDeserializer::new(map))
            }
        }

        // Ignore additive IPC fields while still rejecting a known selector
        // that contradicts the requested kind, including an explicit null.
        let selector = deserializer.deserialize_map(SelectorObject)?;
        match (selector.kind, selector.item, selector.query_ref) {
            (Kind::ScopeRoot, None, None) => Ok(Self::ScopeRoot {}),
            (Kind::Folder, Some(item), None) => Ok(Self::Folder { item }),
            (Kind::QueryResult, None, Some(query_ref)) => Ok(Self::QueryResult { query_ref }),
            _ => Err(serde::de::Error::custom("invalid space target selectors")),
        }
    }
}

/// Requests a total from one pinned result, tolerating additive IPC fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
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
