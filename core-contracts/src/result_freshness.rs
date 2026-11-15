//! Freshness change notifications published by the result store's freshness ledger.
//!
//! The ledger belongs to one scope version and has one row per scope root. This event
//! carries the latest sequence, a whole-ledger summary and the rows that changed.
use crate::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// Event topic for [`ResultFreshnessChanged`].
pub const RESULT_FRESHNESS_CHANGED: &str = "results.freshness_changed";

/// The most ranges one event carries; a producer sends `requery` for more.
pub const MAX_FRESHNESS_RANGES: usize = 256;

const SEQUENCE_LIMIT: u64 = 1_u64 << 53;

/// A freshness state name, an open set of snake_case strings.
///
/// Known names are `observed`, `unwatched`, `change_notified`, `gap_unknown`,
/// `refreshing` and `unverifiable`. The representative state of several rows is the
/// worst one, from worst to best: `unverifiable`, `gap_unknown`, `change_notified`,
/// `refreshing`, `unwatched`, `observed`.
///
/// A later contract version may add names. Deserialization accepts any string, and a
/// receiver must never treat a name it does not know as `observed`; it shows the state
/// as unknown or treats it as the least certain state.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
#[serde(transparent)]
pub struct FreshnessState(String);

impl FreshnessState {
    /// The range was observed and is being watched.
    pub const OBSERVED: &'static str = "observed";
    /// The range was observed and nothing watches it.
    pub const UNWATCHED: &'static str = "unwatched";
    /// A change notification arrived after the observation.
    pub const CHANGE_NOTIFIED: &'static str = "change_notified";
    /// Watching had a gap, so changes may have been missed.
    pub const GAP_UNKNOWN: &'static str = "gap_unknown";
    /// A new observation of the range is in progress.
    pub const REFRESHING: &'static str = "refreshing";
    /// The range could not be observed.
    pub const UNVERIFIABLE: &'static str = "unverifiable";

    /// Wraps a state name without checking it against the known names.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the state name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Row counts per state, a map from state name to row count; the unit is ledger rows,
/// not roots.
///
/// Each key is the state name itself (`observed`, `unwatched`, `change_notified`,
/// `gap_unknown`, `refreshing`, `unverifiable`), so a receiver reads `counts[state]`.
/// A producer always sends all six keys, including zeros. A receiver reads a missing
/// key as 0 and ignores a key it does not know. Only these keys are snake_case; the
/// other fields of the event body are camelCase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case", default)]
pub struct FreshnessCounts {
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub observed: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub unwatched: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub change_notified: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub gap_unknown: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub refreshing: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub unverifiable: u64,
}

impl FreshnessCounts {
    /// The number of ledger rows counted, saturating at `u64::MAX`.
    pub fn total(&self) -> u64 {
        [
            self.observed,
            self.unwatched,
            self.change_notified,
            self.gap_unknown,
            self.refreshing,
            self.unverifiable,
        ]
        .into_iter()
        .fold(0, u64::saturating_add)
    }
}

/// The whole ledger after the change. It is always the full value, so merging events of
/// one scope version keeps the newest summary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FreshnessSummary {
    /// The worst state of all rows.
    ///
    /// A summary without rows has no worst state, and its representative is
    /// `unverifiable`; a receiver never assumes `observed` there. A receiver first
    /// reads an event without a current version and with every count zero as the removal
    /// of that scope version's ledger.
    pub representative: FreshnessState,
    pub counts: FreshnessCounts,
}

/// One ledger row's state after the change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FreshnessRange {
    /// Position in the root list of the event's scope version.
    pub root_index: u32,
    pub state: FreshnessState,
    /// An existing reason key explaining the state, such as a scope gap's reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub reason_key: Option<String>,
}

/// What changed since the previous sequence of the same scope version.
///
/// A receiver applies `ranges` only when the event's `sequence` is exactly the last
/// sequence it received for that scope version plus one. In every other case it treats
/// the event as `requery`: it reads the current state again and applies no ranges.
///
/// A receiver branches only on whether `kind` is `"ranges"` and handles every other
/// value as `requery`. The generated TypeScript lists `"unknown"` for a kind this
/// version does not know; it is not a value a producer sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FreshnessChanged {
    /// The rows that changed. The list is the change since the previous sequence. A range
    /// without a directory reference means its whole root. At most
    /// [`MAX_FRESHNESS_RANGES`] ranges are sent; a producer sends `requery` instead of more.
    Ranges { ranges: Vec<FreshnessRange> },
    /// The receiver reads the state again. A queue that merges events of one scope version
    /// also sends this, because the merged change list is not retained.
    Requery,
    /// Any kind this version does not know; handle it as `requery`. Producers never send it.
    #[serde(rename = "unknown", other)]
    Unknown,
}

/// The topic body. Its sequence belongs to the scope version, not to the transport queue.
///
/// A re-sent copy keeps its sequence and summary and has `changed` set to `requery`.
/// A receiver drops an event whose sequence is not above the last one it received for the
/// scope version, but it accepts the same sequence when it never received an event of that
/// scope version or has just received `job.resync_required`.
///
/// The envelope `seq` is not used to detect gaps on this topic. Gaps and order are
/// judged only by the body `sequence`. The envelope `seq` is the desktop queue's transport
/// counter, shared with other topics; the queue assigns it when it hands an event out, a
/// re-sent copy included (the copy kept for re-sending holds 0 until then).
///
/// A removal event (no current version, every count zero) ends that scope version's
/// ledger, and a receiver forgets its last sequence for it. The producer remembers the
/// sequences of only the 64 most recently removed scope versions, so a scope version
/// pushed out of that list starts again at 1 when it reappears. After
/// `job.resync_required` a receiver accepts the next event of each scope version whatever
/// its sequence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResultFreshnessChanged {
    /// The lineage whose `results.version_changed` events the receiver tracks.
    pub lineage_id: Id,
    /// The scope version whose ledger changed; `rootIndex` refers to its root list.
    pub scope_version_id: Id,
    /// Rises by one for each change of this scope version's ledger, from 1, below 2^53.
    /// It is not comparable with the lineage sequence of `results.version_changed`.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    pub summary: FreshnessSummary,
    pub changed: FreshnessChanged,
    /// The observation time of the result version that caused this change. Omitted when
    /// the change does not come from an observation (a refresh start or a withdrawal) or
    /// that version has no observation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub observed_at: Option<Timestamp>,
}

/// A committed ledger change and the current result version of its scope version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultFreshnessNotice {
    /// The current result version of the scope version. `None` means the scope version has
    /// no current version any more; the ledger is gone, every count is zero and the
    /// representative is `unverifiable`.
    pub result_version: Option<Id>,
    pub body: ResultFreshnessChanged,
}

impl ResultFreshnessNotice {
    /// Checks invariants spanning the envelope and the topic-specific body.
    pub fn validate(&self) -> bool {
        let body = &self.body;
        let total = body.summary.counts.total();
        let requery = matches!(body.changed, FreshnessChanged::Requery);
        let changed_ok = match &body.changed {
            FreshnessChanged::Ranges { ranges } => {
                !ranges.is_empty() && ranges.len() <= MAX_FRESHNESS_RANGES
            }
            FreshnessChanged::Requery => true,
            FreshnessChanged::Unknown => false,
        };
        let current_ok = if self.result_version.is_some() {
            total > 0
        } else {
            total == 0
                && requery
                && body.observed_at.is_none()
                && body.summary.representative.as_str() == FreshnessState::UNVERIFIABLE
        };
        changed_ok && current_ok && body.sequence > 0 && body.sequence < SEQUENCE_LIMIT
    }
}

/// Enqueues a committed freshness notification without invoking its subscribers.
pub trait ResultFreshnessPublisher: Send + Sync {
    /// Queues `notice`; the call never runs subscriber code.
    fn publish_result_freshness(&self, notice: ResultFreshnessNotice);
}

#[cfg(test)]
mod tests;
