//! Read and atomically delete bounded batches of expired job history.

use du_core_contracts::{
    Id,
    job::{
        HistoryPruneCandidate, HistoryPruneDeleteError, HistoryPruneDeleteReceipt,
        HistoryPrunePosition, HistoryPruneRead, HistoryPruneReadError, HistoryRevision,
    },
};
use rusqlite::{Error, ErrorCode, TransactionBehavior, params};

use crate::AppDb;

const JOB_ID_TABLES: &[&str] = &[
    "job_runtime_job_start",
    "job_runtime_item_result",
    "job_runtime_job_interrupt",
    "job_runtime_job_link",
    "job_runtime_job_record",
    "job_runtime_job_scope_root",
    "job_runtime_approval_check",
];

/// F28 retains approval rows without a live job-history link for a fixed 90 days.
const UNLINKED_APPROVAL_RETENTION_DAYS: u32 = 90;

impl AppDb {
    /// Reads one keyset page of expired jobs and their attached row counts.
    pub fn read_history_prune_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
        after: Option<HistoryPrunePosition>,
        limit: usize,
    ) -> Result<Vec<HistoryPruneCandidate>, Error> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).map_err(|_| Error::InvalidQuery)?;
        let cutoff_ms = cutoff(days, reference_ms);
        let after_seq = after
            .map(|position| i64::try_from(position.record_seq))
            .transpose()
            .map_err(|_| Error::InvalidQuery)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let query = candidate_query();
        let mut candidates = Vec::new();
        {
            let mut statement = tx.prepare(&query)?;
            let rows = statement.query_map(
                params![
                    cutoff_ms,
                    reference_ms,
                    after.map(|position| position.reckon_at_ms),
                    after_seq,
                    limit,
                ],
                |row| {
                    Ok((
                        row.get::<_, [u8; 16]>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, bool>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?;
            for row in rows {
                let (raw_id, reckon_at_ms, record_seq, has_nonterminal_follow_up, row_count) = row?;
                let job_id = Id::from_v7_bytes(raw_id);
                if job_id.v7_bytes() != Some(raw_id) {
                    return Err(Error::InvalidQuery);
                }
                candidates.push(HistoryPruneCandidate {
                    job_id,
                    position: HistoryPrunePosition {
                        reckon_at_ms,
                        record_seq: u64::try_from(record_seq).map_err(|_| Error::InvalidQuery)?,
                    },
                    has_nonterminal_follow_up,
                    row_count: u64::try_from(row_count).map_err(|_| Error::InvalidQuery)?,
                });
            }
        }
        tx.commit()?;
        Ok(candidates)
    }

    /// Deletes currently expired, unheld job references in one immediate transaction.
    ///
    /// The purge gate, all history rows, and the deletion generation share one
    /// commit. A stale candidate is left untouched; failures roll the whole batch back.
    pub fn delete_expired_history_batch(
        &mut self,
        candidates: &[HistoryPruneCandidate],
        days: u32,
        reference_ms: i64,
        observed_at_ms: i64,
    ) -> Result<HistoryPruneDeleteReceipt, HistoryPruneDeleteError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_delete_error)?;
        let cutoff_ms = cutoff(days, reference_ms);
        let unlinked_approval_cutoff_ms = cutoff(UNLINKED_APPROVAL_RETENTION_DAYS, reference_ms);
        let mut eligible = Vec::new();
        for candidate in candidates {
            let bytes = candidate
                .job_id
                .v7_bytes()
                .ok_or(HistoryPruneDeleteError::Unavailable)?;
            let record_seq = i64::try_from(candidate.position.record_seq)
                .map_err(|_| HistoryPruneDeleteError::Unavailable)?;
            let still_eligible: bool = tx
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1
                        FROM job_runtime_job_link AS link
                        WHERE link.job_id = ?1
                          AND link.reckon_at_ms = ?2
                          AND link.record_seq = ?3
                          AND link.reckon_at_ms <= ?4
                          AND link.reckon_at_ms <= ?5
                          AND (
                              EXISTS (SELECT 1 FROM job_runtime_job_record WHERE job_id = link.job_id)
                              OR EXISTS (SELECT 1 FROM job_runtime_job_interrupt WHERE job_id = link.job_id)
                          )
                          AND NOT EXISTS (
                              SELECT 1
                              FROM job_runtime_job_link AS successor
                              LEFT JOIN job_runtime_job_record AS successor_record USING (job_id)
                              LEFT JOIN job_runtime_job_interrupt AS successor_interrupted USING (job_id)
                              WHERE successor.predecessor_job_id = link.job_id
                                AND successor_record.job_id IS NULL
                                AND successor_interrupted.job_id IS NULL
                          )
                    )",
                    params![bytes.as_slice(), candidate.position.reckon_at_ms, record_seq, cutoff_ms, reference_ms],
                    |row| row.get(0),
                )
                .map_err(map_delete_error)?;
            if still_eligible {
                eligible.push(bytes);
            }
        }

        let has_expired_unlinked_approval_check: bool = tx
            .query_row(
                "SELECT EXISTS (
                    SELECT 1
                    FROM job_runtime_approval_check AS approval
                    WHERE approval.created_at_ms <= ?1
                      AND (
                          approval.job_id IS NULL
                          OR (
                              NOT EXISTS (
                                  SELECT 1 FROM job_runtime_job_record AS record
                                  WHERE record.job_id = approval.job_id
                              )
                              AND NOT EXISTS (
                                  SELECT 1 FROM job_runtime_job_interrupt AS interrupted
                                  WHERE interrupted.job_id = approval.job_id
                              )
                          )
                      )
                )",
                [unlinked_approval_cutoff_ms],
                |row| row.get(0),
            )
            .map_err(map_delete_error)?;

        if eligible.is_empty() && !has_expired_unlinked_approval_check {
            let revision = read_revision(&tx).map_err(map_delete_error)?;
            tx.commit().map_err(map_delete_error)?;
            return Ok(HistoryPruneDeleteReceipt {
                revision,
                observed_at_ms,
                deleted_job_ids: Vec::new(),
                deleted_row_count: 0,
            });
        }

        let opened = tx
            .execute(
                "UPDATE app_db_purge_gate SET open = 1 WHERE gate_key = 1 AND open = 0",
                [],
            )
            .map_err(map_delete_error)?;
        if opened != 1 {
            return Err(HistoryPruneDeleteError::Unavailable);
        }

        let placeholders = (1..=eligible.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql_params = eligible
            .iter()
            .map(|id| rusqlite::types::Value::Blob(id.to_vec()))
            .collect::<Vec<_>>();
        let mut deleted_row_count = 0_u64;
        let mut deleted_job_ids = Vec::new();
        if !eligible.is_empty() {
            for table in JOB_ID_TABLES {
                let sql = format!("DELETE FROM {table} WHERE job_id IN ({placeholders})");
                let deleted = tx
                    .execute(&sql, rusqlite::params_from_iter(sql_params.iter()))
                    .map_err(map_delete_error)?;
                deleted_row_count = deleted_row_count
                    .checked_add(
                        u64::try_from(deleted).map_err(|_| HistoryPruneDeleteError::Unavailable)?,
                    )
                    .ok_or(HistoryPruneDeleteError::Unavailable)?;
                if *table == "job_runtime_job_link" {
                    if deleted != eligible.len() {
                        return Err(HistoryPruneDeleteError::Unavailable);
                    }
                    deleted_job_ids = eligible.iter().copied().map(Id::from_v7_bytes).collect();
                }
            }
        }

        let deleted_unlinked_approvals = tx
            .execute(
                "DELETE FROM job_runtime_approval_check AS approval
                 WHERE approval.created_at_ms <= ?1
                   AND (
                       approval.job_id IS NULL
                       OR (
                           NOT EXISTS (
                               SELECT 1 FROM job_runtime_job_record AS record
                               WHERE record.job_id = approval.job_id
                           )
                           AND NOT EXISTS (
                               SELECT 1 FROM job_runtime_job_interrupt AS interrupted
                               WHERE interrupted.job_id = approval.job_id
                           )
                       )
                   )",
                [unlinked_approval_cutoff_ms],
            )
            .map_err(map_delete_error)?;
        deleted_row_count = deleted_row_count
            .checked_add(
                u64::try_from(deleted_unlinked_approvals)
                    .map_err(|_| HistoryPruneDeleteError::Unavailable)?,
            )
            .ok_or(HistoryPruneDeleteError::Unavailable)?;

        if deleted_row_count > 0 {
            let updated = tx
                .execute(
                    "UPDATE job_runtime_history_state
                 SET delete_generation = delete_generation + 1,
                     row_version = row_version + 1,
                     updated_at_ms = ?1
                 WHERE state_key = 'history'",
                    [observed_at_ms],
                )
                .map_err(map_delete_error)?;
            if updated != 1 {
                return Err(HistoryPruneDeleteError::Unavailable);
            }
        }
        let closed = tx
            .execute(
                "UPDATE app_db_purge_gate SET open = 0 WHERE gate_key = 1 AND open = 1",
                [],
            )
            .map_err(map_delete_error)?;
        if closed != 1 {
            return Err(HistoryPruneDeleteError::Unavailable);
        }
        let revision = read_revision(&tx).map_err(map_delete_error)?;
        tx.commit().map_err(map_delete_error)?;

        Ok(HistoryPruneDeleteReceipt {
            revision,
            observed_at_ms,
            deleted_job_ids,
            deleted_row_count,
        })
    }
}

impl HistoryPruneRead for AppDb {
    fn expired_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
        after: Option<HistoryPrunePosition>,
        limit: usize,
    ) -> Result<Vec<HistoryPruneCandidate>, HistoryPruneReadError> {
        self.read_history_prune_candidates(days, reference_ms, after, limit)
            .map_err(|_| HistoryPruneReadError::Unavailable)
    }
}

fn candidate_query() -> String {
    let row_counts = JOB_ID_TABLES
        .iter()
        .map(|table| format!("(SELECT count(*) FROM {table} WHERE job_id = link.job_id)"))
        .collect::<Vec<_>>()
        .join(" + ");
    format!(
        "SELECT link.job_id, link.reckon_at_ms, link.record_seq,
                EXISTS (
                    SELECT 1 FROM job_runtime_job_link AS successor
                    LEFT JOIN job_runtime_job_record AS successor_record USING (job_id)
                    LEFT JOIN job_runtime_job_interrupt AS successor_interrupted USING (job_id)
                    WHERE successor.predecessor_job_id = link.job_id
                      AND successor_record.job_id IS NULL
                      AND successor_interrupted.job_id IS NULL
                ), {row_counts}
         FROM job_runtime_job_link AS link INDEXED BY ix_job_runtime_job_link_reckon
         LEFT JOIN job_runtime_job_record AS record USING (job_id)
         LEFT JOIN job_runtime_job_interrupt AS interrupted USING (job_id)
         WHERE link.reckon_at_ms <= ?1
           AND link.reckon_at_ms <= ?2
           AND (record.job_id IS NOT NULL OR interrupted.job_id IS NOT NULL)
           AND (?3 IS NULL OR link.reckon_at_ms > ?3 OR (link.reckon_at_ms = ?3 AND link.record_seq > ?4))
         ORDER BY link.reckon_at_ms ASC, link.record_seq ASC
         LIMIT ?5"
    )
}

fn cutoff(days: u32, reference_ms: i64) -> i64 {
    reference_ms.saturating_sub(i64::from(days).saturating_mul(86_400_000))
}

fn read_revision(tx: &rusqlite::Transaction<'_>) -> Result<HistoryRevision, Error> {
    let (generation, sequence): (i64, i64) = tx.query_row(
        "SELECT delete_generation, last_record_seq FROM job_runtime_history_state WHERE state_key='history'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(HistoryRevision {
        delete_generation: u64::try_from(generation).map_err(|_| Error::InvalidQuery)?,
        max_record_seq: u64::try_from(sequence).map_err(|_| Error::InvalidQuery)?,
    })
}

fn map_delete_error(error: Error) -> HistoryPruneDeleteError {
    match error {
        Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            HistoryPruneDeleteError::Busy
        }
        Error::SqliteFailure(failure, _) if failure.code == ErrorCode::DiskFull => {
            HistoryPruneDeleteError::DiskFull
        }
        _ => HistoryPruneDeleteError::Unavailable,
    }
}

#[cfg(test)]
mod tests;
