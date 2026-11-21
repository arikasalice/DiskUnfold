use crate::{Id, job::HistoryRevision};

/// Stable position used to page expired histories in reckon order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct HistoryPrunePosition {
    /// The history reckon time.
    pub reckon_at_ms: i64,
    /// The globally unique history record sequence used to break time ties.
    pub record_seq: u64,
}

/// One expired history and the storage rows currently attached to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryPruneCandidate {
    /// The persistent job reference.
    pub job_id: Id,
    /// The history position used for paging.
    pub position: HistoryPrunePosition,
    /// Whether a nonterminal successor currently holds the history.
    pub has_nonterminal_follow_up: bool,
    /// The number of rows attached to this job across all job-runtime tables.
    pub row_count: u64,
}

/// Failure to read a bounded page of expired history candidates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryPruneReadError {
    /// The history storage could not provide the requested page.
    Unavailable,
}

/// Reads bounded expired-history pages at a fixed reference time.
pub trait HistoryPruneRead: Send {
    /// Returns at most `limit` expired candidates after `after`, ordered by
    /// `(reckon_at_ms, record_seq)` ascending. Implementations include each
    /// candidate's current history-row count and follow-up hold status.
    fn expired_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
        after: Option<HistoryPrunePosition>,
        limit: usize,
    ) -> Result<Vec<HistoryPruneCandidate>, HistoryPruneReadError>;
}

/// Failure to delete an expired-history batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryPruneDeleteError {
    /// SQLite could not acquire its write lock.
    Busy,
    /// SQLite ran out of disk space.
    DiskFull,
    /// The history storage failed for another reason.
    Unavailable,
}

/// Receipt for a committed history-prune batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryPruneDeleteReceipt {
    /// The history revision visible after the commit.
    pub revision: HistoryRevision,
    /// The UTC time passed to the committed deletion transaction.
    pub observed_at_ms: i64,
    /// Job references whose history-link rows were deleted.
    pub deleted_job_ids: Vec<Id>,
    /// Total rows deleted from all job-runtime history tables.
    pub deleted_row_count: u64,
}

/// Deletes a batch only while the submitted retention policy remains current.
pub trait HistoryPruneDelete: Send + Sync {
    /// Rechecks the expected policy fingerprint while its owner prevents a
    /// concurrent policy update, then revalidates expiry and holds in the
    /// immediate-lock database transaction. `Ok(None)` means the policy
    /// changed or cleanup was disabled and no deletion was attempted.
    fn delete_expired(
        &self,
        expected_policy_fingerprint: &str,
        days: u32,
        reference_ms: i64,
        candidates: &[HistoryPruneCandidate],
        observed_at_ms: i64,
    ) -> Result<Option<HistoryPruneDeleteReceipt>, HistoryPruneDeleteError>;
}
