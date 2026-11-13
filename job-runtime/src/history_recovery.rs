use crate::JobEvents;
use du_core_contracts::job::{HistoryChangeReason, HistoryChanged, HistoryRecovery};

/// Marks operations left by the previous session and notifies only after commit.
///
/// Call before accepting any new per-item work, while the session lock is held.
pub fn recover_history(
    storage: &dyn HistoryRecovery,
    events: &dyn JobEvents,
    now_ms: i64,
) -> Result<(), String> {
    if let Some(receipt) = storage.mark_interrupted(now_ms)? {
        events.publish_history_changed(HistoryChanged {
            revision: receipt.revision,
            reason: HistoryChangeReason::Interrupted,
            count: receipt.count,
            observed_at_ms: receipt.observed_at_ms,
            deleted_job_ids: Vec::new(),
        });
    }
    Ok(())
}
