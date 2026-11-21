use crate::{
    Id,
    history_ipc::{HistoryItemOutcome, HistoryRelation},
};
use std::fmt;

/// Maximum byte length of an opaque per-item history reference.
pub const ITEM_HISTORY_MAX_ITEM_REF_BYTES: usize = 1_024;
/// Maximum character length of a per-item display location.
pub const ITEM_HISTORY_MAX_LOCATION_TEXT_CHARS: usize = 32_768;

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
#[derive(Clone, Eq, PartialEq)]
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

impl fmt::Debug for ItemHistoryRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ItemHistoryRecord")
            .field("item_seq", &self.item_seq)
            .field(
                "item_ref",
                &format_args!("<redacted: {} bytes>", self.item_ref.len()),
            )
            .field(
                "location_raw",
                &format_args!("<redacted: {} bytes>", self.location_raw.len()),
            )
            .field(
                "location_text",
                &format_args!("<redacted: {} chars>", self.location_text.chars().count()),
            )
            .field("outcome", &self.outcome)
            .field("reason_key", &self.reason_key)
            .field("review_needed", &self.review_needed)
            .field("held", &self.held)
            .field("outcome_unknown", &self.outcome_unknown)
            .field("observed_size", &self.observed_size)
            .field("displaced", &self.displaced)
            .finish()
    }
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
#[derive(Clone, Eq, PartialEq)]
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

impl fmt::Debug for DisplacedItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DisplacedItem")
            .field(
                "trash_ref",
                &self
                    .trash_ref
                    .as_ref()
                    .map(|reference| format!("<redacted: {} bytes>", reference.len())),
            )
            .field(
                "name",
                &self
                    .name
                    .as_ref()
                    .map(|name| format!("<redacted: {} chars>", name.chars().count())),
            )
            .field(
                "original_location",
                &self
                    .original_location
                    .as_ref()
                    .map(|location| format!("<redacted: {} chars>", location.chars().count())),
            )
            .field("trashed_at_ms", &self.trashed_at_ms)
            .finish()
    }
}

/// Failure to append a per-item history record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemHistoryError {
    /// The submitted item sequence is greater than the next expected sequence.
    OutOfOrder,
    /// The item sequence was already recorded with different content or is older than the last.
    Duplicate,
    /// The submitted item record is invalid and was not written.
    Invalid,
    /// SQLite could not acquire its bounded write lock.
    Busy,
    /// The history recorder or storage is unavailable.
    Unavailable,
}

impl fmt::Display for ItemHistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OutOfOrder => "item history sequence is out of order",
            Self::Duplicate => "item history sequence is already recorded",
            Self::Invalid => "item history record is invalid",
            Self::Busy => "item history storage is busy",
            Self::Unavailable => "item history recording is unavailable",
        })
    }
}

impl std::error::Error for ItemHistoryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_ipc::HistoryItemOutcome;

    #[test]
    fn item_history_debug_redacts_locations_and_names() {
        let secret_path = r"C:\private\distinctive-item-history-path-206";
        let record = ItemHistoryRecord {
            item_seq: 3,
            item_ref: secret_path.as_bytes().to_vec(),
            location_raw: secret_path.as_bytes().to_vec(),
            location_text: secret_path.to_owned(),
            outcome: HistoryItemOutcome::Failed,
            reason_key: Some("file.access_denied".into()),
            review_needed: true,
            held: false,
            outcome_unknown: false,
            observed_size: ObservedSize::Unknown,
            displaced: Some(DisplacedItem {
                trash_ref: Some(secret_path.as_bytes().to_vec()),
                name: Some(secret_path.to_owned()),
                original_location: Some(secret_path.to_owned()),
                trashed_at_ms: Some(123),
            }),
        };

        let debug = format!("{record:?}");
        assert!(!debug.contains(secret_path));
        assert!(debug.contains("item_seq: 3"));
        assert!(debug.contains("file.access_denied"));
        assert!(debug.contains("redacted"));
    }
}
