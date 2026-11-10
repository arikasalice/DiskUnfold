//! GUI history list request and response contracts.

use crate::{Id, Timestamp, job::HistoryRevision};
use serde::{Deserialize, Serialize};

/// Notifies the main window that a history transaction committed.
pub const HISTORY_CHANGED: &str = "history.changed";

/// A terminal outcome or a persisted interruption marker.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HistoryEndState {
    Done,
    Partial,
    Failed,
    Cancelled,
    Interrupted,
}

impl TryFrom<&str> for HistoryEndState {
    type Error = ();
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "done" => Ok(Self::Done),
            "partial" => Ok(Self::Partial),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(()),
        }
    }
}

/// How a job is related to its predecessor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HistoryRelation {
    Retry,
    Restore,
    MoveUndo,
    EffectCheck,
}

impl TryFrom<&str> for HistoryRelation {
    type Error = ();
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "retry" => Ok(Self::Retry),
            "restore" => Ok(Self::Restore),
            "move_undo" => Ok(Self::MoveUndo),
            "effect_check" => Ok(Self::EffectCheck),
            _ => Err(()),
        }
    }
}

/// Empty category lists do not restrict the query.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryListFilter {
    #[cfg_attr(feature = "typescript", ts(type = "number | null"))]
    pub finished_from_ms: Option<i64>,
    #[cfg_attr(feature = "typescript", ts(type = "number | null"))]
    pub finished_before_ms: Option<i64>,
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default)]
    pub states: Vec<HistoryEndState>,
    #[serde(default)]
    pub relations: Vec<HistoryRelation>,
}

/// Reads the first page or repeats the filter with an authenticated continuation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryListRequest {
    pub filter: Option<HistoryListFilter>,
    pub cursor: Option<String>,
    pub count: Option<u32>,
}

/// Decimal strings preserve the full revision across the JavaScript boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryListRevision {
    pub delete_generation: String,
    pub max_record_seq: String,
}

impl From<HistoryRevision> for HistoryListRevision {
    fn from(revision: HistoryRevision) -> Self {
        Self {
            delete_generation: revision.delete_generation.to_string(),
            max_record_seq: revision.max_record_seq.to_string(),
        }
    }
}

/// Public history change notification. Deleted references stay in the process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryChangedBody {
    /// The committed revision, encoded in the same form as a list page.
    pub revision: HistoryListRevision,
    /// Open reason key for consumers that may lag behind the producer.
    pub reason: String,
    /// The number of affected records as a decimal string.
    pub count: String,
    /// The UTC time observed for the committed change.
    pub observed_at: Timestamp,
}

/// Terminal counts represented as decimal strings without JavaScript rounding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryListCounts {
    pub completed: String,
    pub overwritten: String,
    pub failed: String,
    pub skipped: String,
    pub unprocessed: String,
}

/// A readable terminal or interrupted summary. Unknown values remain absent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryListSummary {
    pub kind_id: String,
    pub kind_version: u32,
    pub scope_summary: Option<String>,
    pub state: HistoryEndState,
    pub counts: Option<HistoryListCounts>,
    #[cfg_attr(feature = "typescript", ts(type = "number | null"))]
    pub started_at_ms: Option<i64>,
    #[cfg_attr(feature = "typescript", ts(type = "number | null"))]
    pub finished_at_ms: Option<i64>,
    pub predecessor_job_ref: Option<Id>,
    pub relation: Option<HistoryRelation>,
    pub session_restore_available: Option<bool>,
}

/// A job can remain on the page even if reading its summary failed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryListRow {
    pub job_ref: Option<Id>,
    pub summary: Option<HistoryListSummary>,
    /// Empty when complete. Each entry is a published omission reason.
    pub missing_reasons: Vec<String>,
}

/// The pinned page and an opaque cursor for its next page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryListResponse {
    pub revision: HistoryListRevision,
    pub rows: Vec<HistoryListRow>,
    pub next_cursor: Option<String>,
}
