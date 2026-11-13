//! Compose the history read port with a separate read-only SQLite connection.

use std::{
    io,
    path::Path,
    sync::{Arc, Mutex},
};

use du_app_db::{AppDb, history_read as db};
use du_core_contracts::{
    Id,
    history_ipc::{HistoryEndState, HistoryListCounts, HistoryListRow, HistoryListSummary},
};
use du_engine_api::history::{Entry, Filter, HistoryRead, Position, ReadError, ReadPage, Revision};

use crate::shared_database::SharedDatabase;

#[derive(Clone)]
pub(super) struct HistoryReader {
    connection: Arc<Mutex<AppDb>>,
    _database: SharedDatabase,
}

impl HistoryReader {
    /// Opens one independent reader while retaining the instance and file pins.
    pub(super) fn open(data_dir: &Path, database: SharedDatabase) -> Result<Self, io::Error> {
        // The GUI owns the writer, so its reader attaches to the live database.
        let connection = AppDb::open_read_only_live(&data_dir.join("diskunfold.db"))
            .map_err(|error| io::Error::other(format!("history.storage_unavailable: {error:?}")))?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            _database: database,
        })
    }
}

impl HistoryRead for HistoryReader {
    type Summary = HistoryListRow;

    fn page(
        &self,
        filter: &Filter,
        revision: Option<Revision>,
        after: Option<Position>,
        count: usize,
    ) -> Result<ReadPage<Self::Summary>, ReadError> {
        let continuation = match (revision, after) {
            (None, None) => None,
            (Some(revision), Some(position)) => Some(db::Continuation {
                revision: db::Revision {
                    delete_generation: checked_counter(revision.delete_generation)?,
                    max_record_seq: checked_counter(revision.max_record_seq)?,
                },
                position: db::Position {
                    reckon_at_ms: position.reckon_at_ms,
                    record_seq: checked_counter(position.record_seq)?,
                },
                filter: filter.clone(),
            }),
            _ => return Err(ReadError::Unavailable),
        };
        let page = self
            .connection
            .lock()
            .map_err(|_| ReadError::Unavailable)?
            .read_history_page(filter.clone(), continuation, count)
            .map_err(|error| match error {
                db::Error::VersionChanged => ReadError::VersionChanged,
                _ => ReadError::Unavailable,
            })?;
        Ok(ReadPage {
            revision: Revision {
                delete_generation: published_counter(page.revision.delete_generation)?,
                max_record_seq: published_counter(page.revision.max_record_seq)?,
            },
            has_more: page.next.is_some(),
            entries: page
                .rows
                .into_iter()
                .map(project_entry)
                .collect::<Result<_, _>>()?,
        })
    }
}

fn checked_counter(value: u64) -> Result<i64, ReadError> {
    i64::try_from(value).map_err(|_| ReadError::Unavailable)
}

fn published_counter(value: i64) -> Result<u64, ReadError> {
    u64::try_from(value).map_err(|_| ReadError::Unavailable)
}

fn project_entry(entry: db::Entry) -> Result<Entry<HistoryListRow>, ReadError> {
    let job_ref = history_id(entry.job_id);
    let mut missing_reasons = Vec::new();
    let summary = match entry.summary.filter(|_| job_ref.is_some()) {
        Some(row) => {
            let predecessor_job_ref = row.predecessor_job_id.and_then(history_id);
            if row.scope_summary.is_none()
                || (row.predecessor_job_id.is_some() && predecessor_job_ref.is_none())
            {
                missing_reasons.push("read_failed".into());
            }
            if row.state == HistoryEndState::Interrupted {
                missing_reasons.push("interrupted".into());
            }
            Some(project_summary(row, predecessor_job_ref))
        }
        None => {
            missing_reasons.push("read_failed".into());
            None
        }
    };
    Ok(Entry {
        position: Position {
            reckon_at_ms: entry.position.reckon_at_ms,
            record_seq: published_counter(entry.position.record_seq)?,
        },
        summary: Some(HistoryListRow {
            job_ref,
            summary,
            missing_reasons,
        }),
    })
}

fn project_summary(row: db::Summary, predecessor_job_ref: Option<Id>) -> HistoryListSummary {
    HistoryListSummary {
        session_restore_available: (row.kind_id == "scan.metadata").then_some(false),
        kind_id: row.kind_id,
        kind_version: row.kind_version,
        scope_summary: row.scope_summary,
        state: row.state,
        counts: row.counts.map(|counts| HistoryListCounts {
            completed: counts.completed.to_string(),
            overwritten: counts.overwritten.to_string(),
            failed: counts.failed.to_string(),
            skipped: counts.skipped.to_string(),
            unprocessed: counts.unprocessed.to_string(),
        }),
        started_at_ms: row.started_at_ms,
        finished_at_ms: row.finished_at_ms,
        predecessor_job_ref,
        relation: row.relation,
    }
}

fn history_id(bytes: [u8; 16]) -> Option<Id> {
    (bytes[6] >> 4 == 7 && bytes[8] >> 6 == 2).then(|| Id::from_v7_bytes(bytes))
}

mod detail;
mod items;

#[cfg(test)]
mod tests;
