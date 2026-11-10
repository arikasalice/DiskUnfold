//! Classified collection observations and the local, pinned failure-list contract.
use crate::{Id, PathBytes, Timestamp};
use serde::{Deserialize, Serialize};

/// Failure-list categories in their deterministic publication order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ScanFailureKind {
    CollectionFailed,
    UnreadRange,
    Uncertain,
    UntraversedLink,
}

/// The collector operation at which an issue was observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ScanFailureStage {
    Metadata,
    OpenDirectory,
    Enumerate,
    Traverse,
}

/// A collection fact retained in one immutable result. Paths are root-relative.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanIssue {
    pub root_index: u32,
    pub relative: Vec<Vec<u8>>,
    pub kind: ScanFailureKind,
    pub stage: ScanFailureStage,
    pub reason_key: String,
    pub observed_at: Option<Timestamp>,
}

/// A local display reference scoped to the response's result version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanFailurePathRef {
    pub root_index: u32,
    pub relative: Vec<PathBytes>,
}

/// One classified issue from the pinned result, including optional display evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanFailureRow {
    pub kind: ScanFailureKind,
    pub stage: ScanFailureStage,
    pub path_ref: ScanFailurePathRef,
    pub reason_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observed_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub display_path: Option<String>,
}

/// Reads the first or a later page of failures from one result and filter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanFailuresRequest {
    pub result_version: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub kind: Option<ScanFailureKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub count: Option<u32>,
}

/// A filtered page and its opaque continuation, if more rows remain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanFailures {
    pub result_version: Id,
    pub rows: Vec<ScanFailureRow>,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub total_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub next_cursor: Option<String>,
}
