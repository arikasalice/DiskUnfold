//! Persist startup interruption markers for unfinished per-item jobs.

use du_core_contracts::job::{HistoryInterruptionReceipt, HistoryRevision};
use rusqlite::{TransactionBehavior, params};

use super::map_error;
use crate::AppDb;

impl AppDb {
    /// Mark started jobs that have neither a terminal record nor an interruption.
    ///
    /// The caller must own the GUI instance lock and run this before starting
    /// per-item jobs. Markers, copied links, and revision updates commit together.
    /// A failure leaves the whole batch unchanged. Repeating the check is safe.
    pub fn mark_interrupted_jobs(
        &mut self,
        now_ms: i64,
    ) -> Result<Option<HistoryInterruptionReceipt>, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_error)?;
        let ids = {
            let mut query = tx.prepare(
                "SELECT s.job_id FROM job_runtime_job_start AS s
                 WHERE NOT EXISTS (SELECT 1 FROM job_runtime_job_record AS r WHERE r.job_id=s.job_id)
                   AND NOT EXISTS (SELECT 1 FROM job_runtime_job_interrupt AS i WHERE i.job_id=s.job_id)
                 ORDER BY s.started_at_ms, s.job_id",
            ).map_err(map_error)?;
            let rows = query
                .query_map([], |row| row.get::<_, [u8; 16]>(0))
                .map_err(map_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(map_error)?
        };
        let mut revision = None;
        for id in &ids {
            tx.execute(
                "INSERT INTO job_runtime_job_interrupt (job_id, detected_at_ms) VALUES (?1, ?2)",
                params![id.as_slice(), now_ms],
            )
            .map_err(map_error)?;
            let (generation, sequence): (i64, i64) = tx
                .query_row(
                    "UPDATE job_runtime_history_state
                 SET last_record_seq=last_record_seq+1, row_version=row_version+1, updated_at_ms=?1
                 WHERE state_key='history' RETURNING delete_generation, last_record_seq",
                    [now_ms],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(map_error)?;
            tx.execute(
                "INSERT INTO job_runtime_job_link
                 (job_id, plan_version_id, plan_lineage_id, plan_hash, predecessor_job_id,
                  relation_kind, record_origin, reckon_at_ms, record_seq, created_at_ms)
                 SELECT job_id, plan_version_id, plan_lineage_id, plan_hash, predecessor_job_id,
                        relation_kind, record_origin, ?2, ?3, ?2
                 FROM job_runtime_job_start WHERE job_id=?1",
                params![id.as_slice(), now_ms, sequence],
            )
            .map_err(map_error)?;
            revision = Some(HistoryRevision {
                delete_generation: u64::try_from(generation)
                    .map_err(|_| "history.record_failed")?,
                max_record_seq: u64::try_from(sequence).map_err(|_| "history.record_failed")?,
            });
        }
        let count = u64::try_from(ids.len()).map_err(|_| "history.record_failed")?;
        tx.commit().map_err(map_error)?;
        Ok(revision.map(|revision| HistoryInterruptionReceipt {
            count,
            revision,
            observed_at_ms: now_ms,
        }))
    }
}

#[cfg(test)]
mod tests;
