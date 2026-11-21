//! Classified collection observations and the local, pinned failure-list contract.
use crate::{Id, PathBytes, Timestamp};
use serde::{Deserialize, Deserializer, Serialize};

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

impl ScanFailureKind {
    /// Whether this issue kind and context are included in the scan lane's failure count.
    ///
    /// Failure events at the open and enumerate stages are unread ranges. The
    /// traverse-stage worker failure uses `scan.unclassified`; admission failures
    /// use `scan.boundary_unverifiable`. Other traverse unread ranges describe a
    /// known scope boundary or a revisit and are not failures.
    pub fn counts_as_lane_failure(self, stage: ScanFailureStage, reason_key: &str) -> bool {
        match self {
            Self::CollectionFailed => true,
            Self::UnreadRange => match stage {
                ScanFailureStage::OpenDirectory | ScanFailureStage::Enumerate => true,
                ScanFailureStage::Traverse => matches!(
                    reason_key,
                    unentered_reason::BOUNDARY_UNVERIFIABLE | "scan.unclassified"
                ),
                ScanFailureStage::Metadata => false,
            },
            Self::Uncertain | Self::UntraversedLink => false,
        }
    }
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

/// Reason keys of entries that the directory admission table did not enter.
///
/// Each is recorded as [`ScanFailureKind::UnreadRange`] at
/// [`ScanFailureStage::Traverse`] (FR1.HD1.app-host.DD1). Only an
/// unverifiable range is a scope gap and lowers the result's completeness.
pub mod unentered_reason {
    /// The entry is on another volume than its parent, including a mount
    /// point. The scope version sets this boundary, so it is not a gap.
    pub const BOUNDARY_OTHER_VOLUME: &str = "scan.boundary_other_volume";
    /// The directory was already collected through another path or by a root
    /// with a lower index, so it is not a gap.
    pub const REVISIT_BLOCKED: &str = "scan.revisit_blocked";
    /// The volume boundary, the exclusion rule or the directory identity
    /// could not be decided. The range is a gap and counts as a failure.
    pub const BOUNDARY_UNVERIFIABLE: &str = "scan.boundary_unverifiable";
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
    /// Human-readable display text; never reopen or compare a path from it.
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
    /// Restricts rows to these zero-based root indexes. Omission selects all roots.
    /// The list must contain 1 to 64 strictly ascending indexes, all below the
    /// pinned result's root count; `null`, empty, duplicate, descending, and
    /// oversized lists are invalid.
    #[serde(default, deserialize_with = "deserialize_root_indexes")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub root_indexes: Option<Vec<u32>>,
    /// Selects exactly issues counted as lane failures: every `collection_failed` row;
    /// `unread_range` rows at `open_directory` or `enumerate`; and traverse-stage unread
    /// rows with reason `scan.boundary_unverifiable` or the worker failure reason
    /// `scan.unclassified`. Omission is false. Known other-volume and revisit ranges,
    /// `uncertain`, and `untraversed_link` rows are excluded. When combined with `kind`,
    /// both filters apply.
    #[serde(default, deserialize_with = "deserialize_failures_only")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub failures_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub count: Option<u32>,
}

fn deserialize_root_indexes<'de, D>(deserializer: D) -> Result<Option<Vec<u32>>, D::Error>
where
    D: Deserializer<'de>,
{
    let indexes = Vec::<u32>::deserialize(deserializer)?;
    if indexes.is_empty() || indexes.len() > 64 || indexes.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(serde::de::Error::custom("invalid rootIndexes"));
    }
    Ok(Some(indexes))
}

fn deserialize_failures_only<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    bool::deserialize(deserializer).map(Some)
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
