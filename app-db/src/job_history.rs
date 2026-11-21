//! Persist a summary-history job's terminal summary and history link in one transaction.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use du_core_contracts::{
    Id,
    job::{
        HistoryAppend, HistoryAppendOutcome, HistoryAppendReceipt, HistoryRevision, JobState,
        JobSummary,
    },
};
use rusqlite::{Error, ErrorCode, OptionalExtension, TransactionBehavior, params};

use crate::{
    AppDb, BUSY_TIMEOUT, HISTORY_SCHEMA_VERSION, OpenError, history_open::refuse_history_migration,
};

pub mod prune;
pub mod read;

impl AppDb {
    /// Atomically appends a terminal summary, its local history link, and the next
    /// history sequence for summary-history jobs or a started per-item job.
    ///
    /// A per-item terminal row must match its immutable start row. Its link copies
    /// plan and predecessor references from that start row. Call through the
    /// shared write connection after applying both history schema definitions.
    /// This method does not create or upgrade the database.
    /// Repeating a fully recorded job returns its existing history without
    /// advancing the sequence. An incomplete record is reported as a failure.
    pub fn append_scan_history(
        &mut self,
        summary: &JobSummary,
        now_ms: i64,
    ) -> Result<HistoryAppend, String> {
        self.append_scan_history_with_receipt(summary, now_ms)
            .map(HistoryAppendOutcome::status)
    }

    /// Returns the revision assigned by the same transaction as the new record.
    /// A duplicate or failed write cannot produce an append receipt.
    pub fn append_scan_history_with_receipt(
        &mut self,
        summary: &JobSummary,
        now_ms: i64,
    ) -> Result<HistoryAppendOutcome, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_error)?;
        if let Some(path) = self.history_commit_path.as_deref() {
            let version: u32 = tx
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(map_error)?;
            if version != HISTORY_SCHEMA_VERSION {
                return Err("history.schema_changed".into());
            }
            if let Err(error) = refuse_history_migration(path) {
                return Err(match error {
                    OpenError::IncompleteCreation => "history.migration_in_progress".into(),
                    _ => "history.record_failed".into(),
                });
            }
        }
        let start = tx
            .query_row(
                "SELECT kind_id, kind_version, scope_summary, started_at_ms
                 FROM job_runtime_job_start WHERE job_id=?1",
                [id_bytes(&summary.job_id)
                    .ok_or("history.record_failed")?
                    .as_slice()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(map_error)?;
        let per_item = match start {
            Some((kind, version, scope, started_at)) => {
                if kind != summary.kind_id
                    || u32::try_from(version).ok() != Some(summary.kind_version)
                    || scope != summary.scope_summary
                    || summary.started_at_ms != Some(started_at)
                {
                    return Err("history.record_failed".into());
                }
                true
            }
            None => false,
        };
        let prepared = Prepared::new(summary, per_item).ok_or("history.record_failed")?;
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
                Ok(HistoryAppendOutcome::Duplicate)
            } else {
                Err("history.record_failed".into())
            };
        }
        for (root_seq, location_text) in summary.scope_roots.iter().take(100).enumerate() {
            tx.execute(
                "INSERT INTO job_runtime_job_scope_root
                     (job_id, root_seq, location_text, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    prepared.job_id.as_slice(),
                    i64::try_from(root_seq).map_err(|_| "history.record_failed")?,
                    location_text,
                    now_ms,
                ],
            )
            .map_err(map_error)?;
        }
        let (generation, sequence): (i64, i64) = tx
            .query_row(
                "UPDATE job_runtime_history_state
                SET last_record_seq = last_record_seq + 1,
                    row_version = row_version + 1, updated_at_ms = ?1
                WHERE state_key = 'history'
                RETURNING delete_generation, last_record_seq",
                [now_ms],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(map_error)?;
        let revision = HistoryRevision {
            delete_generation: u64::try_from(generation).map_err(|_| "history.record_failed")?,
            max_record_seq: u64::try_from(sequence).map_err(|_| "history.record_failed")?,
        };
        tx.execute(
            "INSERT INTO job_runtime_job_link
                 (job_id, plan_version_id, plan_lineage_id, plan_hash, predecessor_job_id,
                  relation_kind, record_origin, reckon_at_ms, record_seq, created_at_ms)
             VALUES (?1,
                     (SELECT plan_version_id FROM job_runtime_job_start WHERE job_id=?1),
                     (SELECT plan_lineage_id FROM job_runtime_job_start WHERE job_id=?1),
                     (SELECT plan_hash FROM job_runtime_job_start WHERE job_id=?1),
                     (SELECT predecessor_job_id FROM job_runtime_job_start WHERE job_id=?1),
                     (SELECT relation_kind FROM job_runtime_job_start WHERE job_id=?1),
                     'this_app', ?2, ?3, ?4)",
            params![
                prepared.job_id.as_slice(),
                summary.finished_at_ms,
                sequence,
                now_ms
            ],
        )
        .map_err(map_error)?;
        tx.commit().map_err(map_error)?;
        Ok(HistoryAppendOutcome::Added(Some(HistoryAppendReceipt {
            revision,
            observed_at_ms: now_ms,
        })))
    }

    /// Appends through the same transaction while allowing Ctrl+C to stop SQLite's
    /// bounded lock wait. `should_abort` is called only while SQLite waits for a lock.
    pub fn append_scan_history_interruptible(
        &mut self,
        summary: &JobSummary,
        now_ms: i64,
        should_abort: impl Fn() -> bool + Send + 'static,
    ) -> Result<HistoryAppend, String> {
        let started = Instant::now();
        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupted_by_handler = Arc::clone(&interrupted);
        self.connection
            .busy_handler(Some(move |_| {
                if should_abort() {
                    interrupted_by_handler.store(true, Ordering::Release);
                    return false;
                }
                if started.elapsed() >= BUSY_TIMEOUT {
                    return false;
                }
                thread::sleep(Duration::from_millis(10));
                true
            }))
            .map_err(map_error)?;

        let result = self.append_scan_history(summary, now_ms);
        let _ = self.connection.busy_timeout(BUSY_TIMEOUT);
        if interrupted.load(Ordering::Acquire) {
            Err("history.interrupted".into())
        } else {
            result
        }
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
    fn new(summary: &JobSummary, per_item: bool) -> Option<Self> {
        let valid_scope = if per_item {
            valid_per_item_scope(&summary.kind_id, &summary.scope_summary)
                && valid_per_item_roots(summary)
        } else {
            valid_record_scope(
                &summary.kind_id,
                summary.kind_version,
                &summary.scope_summary,
            ) && valid_scope_roots(summary)
        };
        if !valid_scope
            || !summary.failure_reason_key.as_deref().is_none_or(|reason| {
                reason == "support.internal_error" || per_item && reason == "history.record_failed"
            })
            || !summary.unprocessed_summary.as_deref().is_none_or(|value| {
                if per_item {
                    valid_per_item_text(value)
                } else {
                    valid_unprocessed(&summary.kind_id, value)
                }
            })
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

/// Accepts only the published path-free scope of a kind with summary history.
fn valid_record_scope(kind_id: &str, kind_version: u32, scope: &str) -> bool {
    match (kind_id, kind_version) {
        ("scan.metadata", 1) => valid_scope(scope),
        ("duplicate.check", 1) => scope == "duplicate.check",
        _ => false,
    }
}

fn valid_per_item_scope(kind_id: &str, scope: &str) -> bool {
    kind_id_version_valid(kind_id) && valid_per_item_text(scope)
}

fn kind_id_version_valid(kind_id: &str) -> bool {
    !kind_id.is_empty()
        && kind_id.len() <= 128
        && kind_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_per_item_text(value: &str) -> bool {
    value.chars().count() <= 32_768
        && !value.chars().any(char::is_control)
        && !value
            .chars()
            .any(|character| character == '/' || character == '\\')
}

fn valid_per_item_roots(summary: &JobSummary) -> bool {
    summary
        .scope_roots
        .iter()
        .all(|root| !root.is_empty() && root.chars().count() <= 32_768)
}

fn valid_scope_roots(summary: &JobSummary) -> bool {
    if !summary
        .scope_roots
        .iter()
        .all(|root| !root.is_empty() && root.chars().count() <= 32_768)
    {
        return false;
    }
    match (summary.kind_id.as_str(), summary.kind_version) {
        ("scan.metadata", 1) => scope_root_count(&summary.scope_summary).is_some_and(|count| {
            if count <= 100 {
                usize::try_from(count).is_ok_and(|count| summary.scope_roots.len() == count)
            } else {
                summary.scope_roots.len() >= 100
            }
        }),
        ("duplicate.check", 1) => summary.scope_roots.is_empty(),
        _ => false,
    }
}

fn valid_scope(value: &str) -> bool {
    scope_root_count(value).is_some()
}

fn scope_root_count(value: &str) -> Option<u32> {
    if value == "metadata: 1 root" {
        return Some(1);
    }
    value
        .strip_prefix("metadata: ")
        .and_then(|s| s.strip_suffix(" roots"))
        .and_then(|n| {
            n.parse::<u32>()
                .ok()
                .filter(|count| *count > 1 && count.to_string() == n)
        })
}

fn valid_unprocessed(kind_id: &str, value: &str) -> bool {
    let count = match kind_id {
        "scan.metadata" => value.strip_prefix("unprocessed_roots="),
        "duplicate.check" => value.strip_suffix(" selected items lack final disposition"),
        _ => None,
    };
    count.is_some_and(|n| n.parse::<u64>().is_ok_and(|value| value.to_string() == n))
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

mod interruption;
mod item;
#[cfg(test)]
mod tests;
