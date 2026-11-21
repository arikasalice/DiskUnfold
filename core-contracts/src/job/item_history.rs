use crate::{
    Id,
    history_ipc::{HistoryItemOutcome, HistoryRelation},
};
use std::fmt;

/// Immutable facts captured when a per-item job enters the running state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemHistoryStart {
    /// Stable job identifier.
    pub job_id: Id,
    /// Registered job kind identifier.
    pub kind_id: String,
    /// Registered job kind version.
    pub kind_version: u32,
    /// Path-free scope summary captured at submission.
    pub scope_summary: String,
    /// UTC start time in milliseconds since the Unix epoch.
    pub started_at_ms: i64,
    /// Plan version associated with the job, when supplied.
    pub plan_version_id: Option<Id>,
    /// Plan lineage associated with the job, when supplied.
    pub plan_lineage_id: Option<Id>,
    /// Opaque plan hash, when supplied.
    pub plan_hash: Option<Vec<u8>>,
    /// Related predecessor job, when supplied.
    pub predecessor_job_id: Option<Id>,
    /// Relation to the predecessor, present with `predecessor_job_id`.
    pub relation_kind: Option<HistoryRelation>,
}

/// One immutable per-item outcome reported by a running job body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemHistoryRecord {
    /// Zero-based item sequence, increasing by one for each successful append.
    pub item_seq: u64,
    /// Non-empty opaque item identity; never exposed by history reads.
    pub item_ref: Vec<u8>,
    /// Original location bytes; never exposed by history reads.
    pub location_raw: Vec<u8>,
    /// Display location captured when the result was confirmed.
    pub location_text: String,
    /// Persisted item outcome shared with history reads.
    pub outcome: HistoryItemOutcome,
    /// Stable reason key, when the item has one.
    pub reason_key: Option<String>,
    /// Whether the item needs user review.
    pub review_needed: bool,
    /// Whether a protection rule held the item.
    pub held: bool,
    /// Whether the outcome could not be determined.
    pub outcome_unknown: bool,
    /// Size evidence captured for the item.
    pub observed_size: ObservedSize,
    /// Item displaced to trash, when the outcome permits displacement evidence.
    pub displaced: Option<DisplacedItem>,
}

/// Size evidence retained for one item result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservedSize {
    /// The exact observed byte count.
    Exact(u64),
    /// A lower bound on the observed byte count.
    LowerBound(u64),
    /// No usable size observation was available.
    Unknown,
}

/// Display and opaque reference data for a displaced item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplacedItem {
    /// Opaque trash reference, when available.
    pub trash_ref: Option<Vec<u8>>,
    /// Displaced item name, when available.
    pub name: Option<String>,
    /// Original display location, when available.
    pub original_location: Option<String>,
    /// UTC trash time in milliseconds since the Unix epoch, when available.
    pub trashed_at_ms: Option<i64>,
}

/// Failure to append a per-item history record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemHistoryError {
    /// The submitted item sequence is greater than the next expected sequence.
    OutOfOrder,
    /// The item sequence was already recorded by this job context.
    Duplicate,
    /// SQLite could not acquire its bounded write lock.
    Busy,
    /// The job kind or item result cannot be recorded, or storage failed.
    Unavailable,
}

impl fmt::Display for ItemHistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OutOfOrder => "item history sequence is out of order",
            Self::Duplicate => "item history sequence is already recorded",
            Self::Busy => "item history storage is busy",
            Self::Unavailable => "item history recording is unavailable",
        })
    }
}

impl std::error::Error for ItemHistoryError {}
