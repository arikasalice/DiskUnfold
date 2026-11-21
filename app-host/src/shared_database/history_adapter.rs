use du_core_contracts::job::{
    HistoryAppend, HistoryAppendOutcome, ItemHistoryError, ItemHistoryRecord, ItemHistoryStart,
    JobHistory, JobSummary,
};

use super::SharedDatabase;

/// Local adapter for terminal summaries and F21 per-item history writes.
pub(crate) struct SharedHistory {
    pub(super) database: SharedDatabase,
    pub(super) now_ms: fn() -> i64,
}

impl JobHistory for SharedHistory {
    fn append(&self, summary: &JobSummary) -> Result<HistoryAppend, String> {
        self.append_with_receipt(summary)
            .map(HistoryAppendOutcome::status)
    }

    fn append_with_receipt(&self, summary: &JobSummary) -> Result<HistoryAppendOutcome, String> {
        let now_ms = (self.now_ms)();
        self.database
            .0
            .connection
            .lock()
            .map_err(|_| "history.record_failed".to_owned())?
            .append_scan_history_with_receipt(summary, now_ms)
            .map_err(|reason| {
                eprintln!("history.record_failed: {reason}");
                "history.record_failed".to_owned()
            })
    }

    fn append_item_start(&self, start: &ItemHistoryStart) -> Result<(), ItemHistoryError> {
        let now_ms = (self.now_ms)();
        self.database
            .0
            .connection
            .lock()
            .map_err(|_| ItemHistoryError::Unavailable)?
            .append_item_history_start(start, now_ms)
            .inspect_err(|error| {
                eprintln!("history.item_start_failed: {error:?}");
            })
    }

    fn append_item_result(
        &self,
        job_id: &du_core_contracts::Id,
        item: &ItemHistoryRecord,
    ) -> Result<(), ItemHistoryError> {
        let now_ms = (self.now_ms)();
        self.database
            .0
            .connection
            .lock()
            .map_err(|_| ItemHistoryError::Unavailable)?
            .append_item_history_result(job_id, item, now_ms)
            .inspect_err(|error| {
                eprintln!("history.item_result_failed: {error:?}");
            })
    }
}
