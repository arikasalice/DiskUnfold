//! Persist a metadata scan's terminal summary and history link in one transaction.

use du_core_contracts::{
    Id,
    job::{HistoryAppend, JobState, JobSummary},
};
use rusqlite::{Error, ErrorCode, TransactionBehavior, params};

use crate::AppDb;

impl AppDb {
    /// Atomically appends the path-free terminal summary, its local history link,
    /// and the next history sequence for a metadata scan.
    ///
    /// Call through the shared write connection after applying both history
    /// schema definitions. This method does not create or upgrade the database.
    /// Repeating a fully recorded job returns its existing history without
    /// advancing the sequence. An incomplete record is reported as a failure.
    pub fn append_scan_history(
        &mut self,
        summary: &JobSummary,
        now_ms: i64,
    ) -> Result<HistoryAppend, String> {
        let prepared = Prepared::new(summary).ok_or("history.record_failed")?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_error)?;
        let [completed, overwritten, failed, skipped, unprocessed] = prepared.counts;
        let inserted = tx
            .execute(
                "INSERT INTO job_runtime_job_record
                    (job_id, kind_id, kind_version, scope_summary, final_state,
                     completed_count, overwritten_count, failed_count, skipped_count, unprocessed_count,
                     failure_reason_key, unprocessed_summary, result_version_id,
                     started_at_ms, finished_at_ms, correlation_id, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                 ON CONFLICT(job_id) DO NOTHING",
                params![
                    prepared.job_id.as_slice(),
                    summary.kind_id,
                    summary.kind_version,
                    summary.scope_summary,
                    prepared.state,
                    completed,
                    overwritten,
                    failed,
                    skipped,
                    unprocessed,
                    summary.failure_reason_key,
                    summary.unprocessed_summary,
                    prepared.result_id.as_ref().map(|id| id.as_slice()),
                    summary.started_at_ms,
                    summary.finished_at_ms,
                    prepared.correlation_id.as_slice(),
                    now_ms,
                ],
            )
            .map_err(map_error)?;
        if inserted == 0 {
            let complete: bool = tx
                .query_row(
                    "SELECT EXISTS (
                    SELECT 1 FROM job_runtime_job_link AS link
                    JOIN job_runtime_history_state AS state ON state.state_key = 'history'
                    WHERE link.job_id = ?1 AND link.record_seq <= state.last_record_seq
                )",
                    [prepared.job_id.as_slice()],
                    |row| row.get(0),
                )
                .map_err(map_error)?;
            return if complete {
                Ok(HistoryAppend::Duplicate)
            } else {
                Err("history.record_failed".into())
            };
        }
        let sequence: i64 = tx
            .query_row(
                "UPDATE job_runtime_history_state
                SET last_record_seq = last_record_seq + 1,
                    row_version = row_version + 1, updated_at_ms = ?1
                WHERE state_key = 'history'
                RETURNING last_record_seq",
                [now_ms],
                |row| row.get(0),
            )
            .map_err(map_error)?;
        tx.execute(
            "INSERT INTO job_runtime_job_link
                 (job_id, record_origin, reckon_at_ms, record_seq, created_at_ms)
             VALUES (?1, 'this_app', ?2, ?3, ?4)",
            params![
                prepared.job_id.as_slice(),
                summary.finished_at_ms,
                sequence,
                now_ms
            ],
        )
        .map_err(map_error)?;
        tx.commit().map_err(map_error)?;
        Ok(HistoryAppend::Added)
    }
}

struct Prepared {
    job_id: [u8; 16],
    result_id: Option<[u8; 16]>,
    correlation_id: [u8; 16],
    state: &'static str,
    counts: [i64; 5],
}

impl Prepared {
    fn new(summary: &JobSummary) -> Option<Self> {
        if summary.kind_id != "scan.metadata"
            || summary.kind_version != 1
            || !valid_scope(&summary.scope_summary)
            || !summary
                .failure_reason_key
                .as_deref()
                .is_none_or(|reason| reason == "support.internal_error")
            || !summary
                .unprocessed_summary
                .as_deref()
                .is_none_or(valid_unprocessed)
        {
            return None;
        }
        let state = match summary.state {
            JobState::Done => "done",
            JobState::Partial => "partial",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
            _ => return None,
        };
        Some(Self {
            job_id: id_bytes(&summary.job_id)?,
            result_id: match &summary.result_version_id {
                Some(id) => Some(id_bytes(id)?),
                None => None,
            },
            correlation_id: id_bytes(&summary.correlation_id)?,
            state,
            counts: [
                i64::try_from(summary.counts.completed).ok()?,
                i64::try_from(summary.counts.overwritten).ok()?,
                i64::try_from(summary.counts.failed).ok()?,
                i64::try_from(summary.counts.skipped).ok()?,
                i64::try_from(summary.counts.unprocessed).ok()?,
            ],
        })
    }
}

fn id_bytes(id: &Id) -> Option<[u8; 16]> {
    Some(*uuid::Uuid::parse_str(id.as_str()).ok()?.as_bytes())
}

fn valid_scope(value: &str) -> bool {
    if value == "metadata: 1 root" {
        return true;
    }
    value
        .strip_prefix("metadata: ")
        .and_then(|s| s.strip_suffix(" roots"))
        .is_some_and(|n| {
            n.parse::<u32>()
                .is_ok_and(|value| value > 1 && value.to_string() == n)
        })
}

fn valid_unprocessed(value: &str) -> bool {
    value
        .strip_prefix("unprocessed_roots=")
        .is_some_and(|n| n.parse::<u64>().is_ok_and(|value| value.to_string() == n))
}

fn map_error(error: Error) -> String {
    match error {
        Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            "db.busy"
        }
        Error::SqliteFailure(failure, _) if failure.code == ErrorCode::DiskFull => "db.disk_full",
        _ => "history.record_failed",
    }
    .into()
}

#[cfg(test)]
mod tests;
