//! Runtime clock and committed history notifications for retention pruning.

use du_core_contracts::job::{
    HistoryChangeReason, HistoryChanged, HistoryPruneCandidate, HistoryPruneDelete,
    HistoryPruneDeleteError, HistoryPruneDeleteReceipt, HistoryPrunePosition, HistoryPruneRead,
    HistoryPruneReadError,
};

use crate::JobRuntime;

impl JobRuntime {
    /// Reads the single reference time used by one history-prune job.
    pub fn history_prune_reference_time(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Reads one bounded expiry page at the job's fixed reference time.
    pub fn history_prune_candidates(
        &self,
        history: &mut dyn HistoryPruneRead,
        days: u32,
        reference_ms: i64,
        after: Option<HistoryPrunePosition>,
        limit: usize,
    ) -> Result<Vec<HistoryPruneCandidate>, HistoryPruneReadError> {
        history.expired_candidates(days, reference_ms, after, limit)
    }

    /// Commits one batch and publishes its invalidation only after the commit.
    pub fn prune_history_batch(
        &self,
        history: &dyn HistoryPruneDelete,
        expected_policy_fingerprint: &str,
        days: u32,
        reference_ms: i64,
        candidates: &[HistoryPruneCandidate],
    ) -> Result<Option<HistoryPruneDeleteReceipt>, HistoryPruneDeleteError> {
        let observed_at_ms = self.clock.now_ms();
        history.delete_expired(
            expected_policy_fingerprint,
            days,
            reference_ms,
            candidates,
            observed_at_ms,
        )
    }

    /// Publishes one committed prune receipt after its session state is reconciled.
    pub fn publish_history_prune(&self, receipt: HistoryPruneDeleteReceipt) {
        if receipt.deleted_row_count > 0 {
            self.events.publish_history_changed(HistoryChanged {
                revision: receipt.revision,
                reason: HistoryChangeReason::RetentionPrune,
                count: receipt.deleted_job_ids.len() as u64,
                observed_at_ms: receipt.observed_at_ms,
                deleted_job_ids: receipt.deleted_job_ids,
            });
        }
    }
}
