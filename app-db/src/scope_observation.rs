//! SQLite storage for F03 scan-completion observations.
//!
//! Analysis-index owns what a row means; this module only stores it. Rows are
//! append-only: the schema forbids updates, and deletes pass only while the shared
//! purge gate is open. The retention trim below opens the gate for exactly its own
//! deletes and closes it before the transaction commits, so a failure rolls both back.

use du_core_contracts::{
    Id,
    scope_observation_log::{ScopeObservation, ScopeObservationAppend, ScopeObservationWriteError},
};
use rusqlite::{ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::AppDb;

mod decode;
mod read;
mod row;

/// The schema version that introduced the observation table.
pub const SCOPE_OBSERVATION_SCHEMA_VERSION: u32 = 7;

/// Rows kept for one scope fingerprint and format; older rows are deleted first.
pub const PER_SCOPE_LIMIT: usize = 2_000;

/// Rows kept across all scopes; older rows are deleted first.
pub const TOTAL_LIMIT: usize = 100_000;

/// The newest row that retention must delete: the first one past the per-scope limit.
/// It reads at most `per_scope + 1` entries of the scope's index range.
const CUTOFF_SQL: &str =
    "SELECT observed_at_ms, observation_id FROM analysis_index_scope_observation
     WHERE scope_fingerprint = ?1 AND fingerprint_format = ?2
     ORDER BY observed_at_ms DESC, observation_id DESC LIMIT 1 OFFSET ?3";

/// Deletes one scope's rows at or before the cutoff, as one range of its index.
const DELETE_SCOPE_SQL: &str = "DELETE FROM analysis_index_scope_observation
     WHERE scope_fingerprint = ?1 AND fingerprint_format = ?2
       AND (observed_at_ms, observation_id) <= (?3, ?4)";

/// Counts all rows by walking the smallest index once.
const COUNT_SQL: &str = "SELECT count(*) FROM analysis_index_scope_observation";

/// Deletes the oldest rows across all scopes. It runs only when the table is over its limit.
const DELETE_TOTAL_SQL: &str =
    "DELETE FROM analysis_index_scope_observation WHERE observation_id IN (
         SELECT observation_id FROM analysis_index_scope_observation
         ORDER BY observed_at_ms ASC, observation_id ASC LIMIT ?1)";

/// Retention limits applied inside every append transaction.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Retention {
    pub(crate) per_scope: usize,
    pub(crate) total: usize,
}

impl Retention {
    pub(crate) const DEFAULT: Self = Self {
        per_scope: PER_SCOPE_LIMIT,
        total: TOTAL_LIMIT,
    };
}

impl AppDb {
    /// Appends one observation and trims retention in the same immediate transaction.
    ///
    /// Call through the shared GUI writer of a database upgraded to schema version
    /// seven. This method never creates or upgrades the table. A row for a job that
    /// already has one is left unchanged and reported as already recorded.
    ///
    /// One call is one attempt and waits at most the connection's busy timeout. A busy
    /// database is reported as such and the attempt has rolled back completely, so the
    /// caller may call again without duplicating a row or a deletion. The caller owns
    /// any retry policy, which leaves the connection free for other writers between
    /// attempts.
    pub fn append_scope_observation(
        &mut self,
        observation: &ScopeObservation,
    ) -> Result<ScopeObservationAppend, ScopeObservationWriteError> {
        self.append_scope_observation_with(observation, Retention::DEFAULT)
    }

    pub(crate) fn append_scope_observation_with(
        &mut self,
        observation: &ScopeObservation,
        retention: Retention,
    ) -> Result<ScopeObservationAppend, ScopeObservationWriteError> {
        let encoded = row::encode(observation).ok_or(ScopeObservationWriteError::Invalid)?;
        self.try_append(&encoded, retention)
            .map_err(|error| write_error(&error))
    }

    fn try_append(
        &mut self,
        encoded: &row::Encoded,
        retention: Retention,
    ) -> rusqlite::Result<ScopeObservationAppend> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let recorded: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM analysis_index_scope_observation WHERE job_id = ?1)",
            [encoded.job_id.as_slice()],
            |row| row.get(0),
        )?;
        if recorded {
            return Ok(ScopeObservationAppend::AlreadyRecorded);
        }
        encoded.insert(&tx)?;
        trim(&tx, encoded, retention)?;
        tx.commit()?;
        Ok(ScopeObservationAppend::Added)
    }
}

/// Deletes every observation. The caller is the history purge transaction.
///
/// The purge gate must already be open in `tx`, exactly as for the job history
/// tables it clears, so the whole purge commits or rolls back together. Automatic
/// cleanup of history before a cutoff time must not call this.
pub fn delete_all_scope_observations(tx: &Transaction<'_>) -> rusqlite::Result<usize> {
    tx.execute("DELETE FROM analysis_index_scope_observation", [])
}

/// Deletes the observation of one job and returns how many rows went, zero or one.
///
/// The caller is the transaction that deletes that job's history because the user asked
/// for it (F21 `history.delete`). That deletion must call this for every job it removes,
/// so an analysis the user deleted leaves no point in the trend. The purge gate must
/// already be open in `tx`, exactly as for the job history rows of the same job, so the
/// history and the observation go together or not at all. A job without an observation,
/// or an identifier that is not a job identifier, deletes nothing.
///
/// Automatic cleanup of history before a cutoff time (`history.prune`) must not call
/// this; there the retention limits alone bound the table.
pub fn delete_scope_observation_of_job(
    tx: &Transaction<'_>,
    job_id: &Id,
) -> rusqlite::Result<usize> {
    let Some(job) = job_id.v7_bytes() else {
        return Ok(0);
    };
    tx.execute(
        "DELETE FROM analysis_index_scope_observation WHERE job_id = ?1",
        params![job.as_slice()],
    )
}

/// Deletes rows beyond the per-scope limit, then beyond the total limit, oldest first.
///
/// The per-scope step reads at most `per_scope + 1` index entries of one scope. The total
/// step counts the index once and sorts only when the table is over its limit.
fn trim(
    tx: &Transaction<'_>,
    encoded: &row::Encoded,
    retention: Retention,
) -> rusqlite::Result<()> {
    let per_scope = i64::try_from(retention.per_scope).unwrap_or(i64::MAX);
    let cutoff: Option<(i64, Vec<u8>)> = tx
        .query_row(
            CUTOFF_SQL,
            params![encoded.fingerprint.as_slice(), encoded.format, per_scope],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((observed_at, id)) = cutoff {
        with_open_gate(tx, || {
            tx.execute(
                DELETE_SCOPE_SQL,
                params![
                    encoded.fingerprint.as_slice(),
                    encoded.format,
                    observed_at,
                    id
                ],
            )
            .map(|_| ())
        })?;
    }
    let count: i64 = tx.query_row(COUNT_SQL, [], |row| row.get(0))?;
    let total = i64::try_from(retention.total).unwrap_or(i64::MAX);
    if count > total {
        with_open_gate(tx, || {
            tx.execute(DELETE_TOTAL_SQL, [count - total]).map(|_| ())
        })?;
    }
    Ok(())
}

fn with_open_gate(
    tx: &Transaction<'_>,
    delete: impl FnOnce() -> rusqlite::Result<()>,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE app_db_purge_gate SET open = 1 WHERE gate_key = 1",
        [],
    )?;
    delete()?;
    tx.execute(
        "UPDATE app_db_purge_gate SET open = 0 WHERE gate_key = 1",
        [],
    )?;
    Ok(())
}

fn write_error(error: &rusqlite::Error) -> ScopeObservationWriteError {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
            ScopeObservationWriteError::Busy
        }
        Some(ErrorCode::DiskFull) => ScopeObservationWriteError::DiskFull,
        // A row the table's own constraints refuse; a trigger abort is a storage failure.
        Some(ErrorCode::ConstraintViolation)
            if error.sqlite_error().map(|e| e.extended_code)
                != Some(rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER) =>
        {
            ScopeObservationWriteError::Invalid
        }
        _ => ScopeObservationWriteError::Storage,
    }
}

#[cfg(test)]
mod tests;
