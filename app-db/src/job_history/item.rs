use std::path::Path;

use du_core_contracts::{
    Id,
    history_ipc::{HistoryItemOutcome, HistoryRelation},
    job::{
        DisplacedItem, ITEM_HISTORY_MAX_ITEM_REF_BYTES, ITEM_HISTORY_MAX_LOCATION_TEXT_CHARS,
        ItemHistoryError, ItemHistoryRecord, ItemHistoryStart, ObservedSize,
    },
};
use rusqlite::{Error, ErrorCode, Transaction, TransactionBehavior, params};

use super::{id_bytes, valid_per_item_scope};
use crate::{AppDb, HISTORY_SCHEMA_VERSION, history_open::refuse_history_migration};

impl AppDb {
    /// Appends the immutable start row for one per-item job.
    ///
    /// The caller must invoke this after transitioning the job to running and
    /// before its body executes. This method does not create or upgrade a DB.
    pub fn append_item_history_start(
        &mut self,
        start: &ItemHistoryStart,
        now_ms: i64,
    ) -> Result<(), ItemHistoryError> {
        let job_id = id_bytes(&start.job_id).ok_or(ItemHistoryError::Unavailable)?;
        let plan_version = optional_id_bytes(start.plan_version_id.as_ref())?;
        let plan_lineage = optional_id_bytes(start.plan_lineage_id.as_ref())?;
        let predecessor = optional_id_bytes(start.predecessor_job_id.as_ref())?;
        if start.kind_version == 0
            || !valid_per_item_scope(&start.kind_id, &start.scope_summary)
            || start
                .plan_hash
                .as_ref()
                .is_some_and(|hash| hash.len() != 32)
            || start.predecessor_job_id.is_some() != start.relation_kind.is_some()
        {
            return Err(ItemHistoryError::Unavailable);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_item_error)?;
        check_schema(&tx, self.history_commit_path.as_deref())?;
        tx.execute(
            "INSERT INTO job_runtime_job_start
                 (job_id, kind_id, kind_version, scope_summary, started_at_ms,
                  plan_version_id, plan_lineage_id, plan_hash, predecessor_job_id,
                  relation_kind, record_origin, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'this_app', ?11)",
            params![
                job_id.as_slice(),
                start.kind_id,
                start.kind_version,
                start.scope_summary,
                start.started_at_ms,
                plan_version.as_ref().map(|value| value.as_slice()),
                plan_lineage.as_ref().map(|value| value.as_slice()),
                start.plan_hash.as_deref(),
                predecessor.as_ref().map(|value| value.as_slice()),
                start.relation_kind.map(relation_name),
                now_ms,
            ],
        )
        .map_err(map_item_error)?;
        tx.commit().map_err(map_item_error)
    }

    /// Appends one item result in its own immediate transaction.
    ///
    /// Item sequences must begin at zero and be contiguous. The append-only
    /// schema and its CHECK constraints remain the final storage boundary.
    pub fn append_item_history_result(
        &mut self,
        job_id: &Id,
        item: &ItemHistoryRecord,
        now_ms: i64,
    ) -> Result<(), ItemHistoryError> {
        let job_key = id_bytes(job_id).ok_or(ItemHistoryError::Unavailable)?;
        let prepared = PreparedItem::new(item)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_item_error)?;
        check_schema(&tx, self.history_commit_path.as_deref())?;
        let started: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM job_runtime_job_start WHERE job_id=?1)",
                [job_key.as_slice()],
                |row| row.get(0),
            )
            .map_err(map_item_error)?;
        if !started {
            return Err(ItemHistoryError::Unavailable);
        }
        let terminal: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM job_runtime_job_record WHERE job_id=?1)",
                [job_key.as_slice()],
                |row| row.get(0),
            )
            .map_err(map_item_error)?;
        if terminal {
            return Err(ItemHistoryError::Unavailable);
        }
        let next_sequence: i64 = tx
            .query_row(
                "SELECT COALESCE(max(item_seq) + 1, 0)
                 FROM job_runtime_item_result WHERE job_id=?1",
                [job_key.as_slice()],
                |row| row.get(0),
            )
            .map_err(map_item_error)?;
        if prepared.sequence < next_sequence {
            return Err(ItemHistoryError::Duplicate);
        }
        if prepared.sequence > next_sequence {
            return Err(ItemHistoryError::OutOfOrder);
        }
        tx.execute(
            "INSERT INTO job_runtime_item_result
                 (job_id, item_seq, item_ref, location_raw, location_text, outcome,
                  reason_key, review_needed, held, outcome_unknown, observed_size,
                  observed_size_state, displaced_to_trash, displaced_trash_ref,
                  displaced_name, displaced_original_location, displaced_trashed_at_ms,
                  created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                job_key.as_slice(),
                prepared.sequence,
                item.item_ref,
                item.location_raw,
                item.location_text,
                item.outcome.as_str(),
                item.reason_key,
                i64::from(item.review_needed),
                i64::from(item.held),
                i64::from(item.outcome_unknown),
                prepared.size,
                prepared.size_state,
                i64::from(prepared.displaced.is_some()),
                prepared.displaced.as_ref().and_then(|item| item.trash_ref.as_deref()),
                prepared.displaced.as_ref().and_then(|item| item.name.as_deref()),
                prepared.displaced.as_ref().and_then(|item| item.original_location.as_deref()),
                prepared.displaced.as_ref().and_then(|item| item.trashed_at_ms),
                now_ms,
            ],
        )
        .map_err(map_item_error)?;
        tx.commit().map_err(map_item_error)
    }
}

struct PreparedItem<'a> {
    sequence: i64,
    size: Option<i64>,
    size_state: &'static str,
    displaced: Option<&'a DisplacedItem>,
}

impl<'a> PreparedItem<'a> {
    fn new(item: &'a ItemHistoryRecord) -> Result<Self, ItemHistoryError> {
        if item.item_ref.is_empty()
            || item.item_ref.len() > ITEM_HISTORY_MAX_ITEM_REF_BYTES
            || item.location_text.chars().count() > ITEM_HISTORY_MAX_LOCATION_TEXT_CHARS
            || item.displaced.as_ref().is_some_and(|displaced| {
                displaced.name.as_deref() == Some("")
                    || displaced.original_location.as_deref() == Some("")
            })
            || (item.outcome == HistoryItemOutcome::Overwritten && item.displaced.is_none())
            || (item.displaced.is_some()
                && !matches!(
                    item.outcome,
                    HistoryItemOutcome::Overwritten | HistoryItemOutcome::Failed
                ))
        {
            return Err(ItemHistoryError::Invalid);
        }
        let (size, size_state) = match item.observed_size {
            ObservedSize::Exact(value) => (Some(size_value(value)?), "exact"),
            ObservedSize::LowerBound(value) => (Some(size_value(value)?), "lower_bound"),
            ObservedSize::Unknown => (None, "unknown"),
        };
        Ok(Self {
            sequence: i64::try_from(item.item_seq).map_err(|_| ItemHistoryError::Invalid)?,
            size,
            size_state,
            displaced: item.displaced.as_ref(),
        })
    }
}

fn size_value(value: u64) -> Result<i64, ItemHistoryError> {
    i64::try_from(value).map_err(|_| ItemHistoryError::Invalid)
}

fn optional_id_bytes(id: Option<&Id>) -> Result<Option<[u8; 16]>, ItemHistoryError> {
    id.map(|id| id_bytes(id).ok_or(ItemHistoryError::Unavailable))
        .transpose()
}

fn check_schema(tx: &Transaction<'_>, commit_path: Option<&Path>) -> Result<(), ItemHistoryError> {
    let version: u32 = tx
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(map_item_error)?;
    if version != HISTORY_SCHEMA_VERSION {
        return Err(ItemHistoryError::Unavailable);
    }
    if let Some(path) = commit_path
        && refuse_history_migration(path).is_err()
    {
        return Err(ItemHistoryError::Unavailable);
    }
    Ok(())
}

fn relation_name(relation: HistoryRelation) -> &'static str {
    match relation {
        HistoryRelation::Retry => "retry",
        HistoryRelation::Restore => "restore",
        HistoryRelation::MoveUndo => "move_undo",
        HistoryRelation::EffectCheck => "effect_check",
    }
}

fn map_item_error(error: Error) -> ItemHistoryError {
    match error {
        Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            ItemHistoryError::Busy
        }
        _ => ItemHistoryError::Unavailable,
    }
}

#[cfg(test)]
mod tests;
