//! Read immutable item results from a revision-fenced SQLite snapshot.

use du_core_contracts::{
    ByteSize, Id,
    history_ipc::{HistoryDisplacedItem, HistoryItemResult},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::Revision;
use crate::AppDb;

pub use du_core_contracts::history_ipc::HistoryItemOutcome as Outcome;

/// A continuation must be authenticated by the transport before use.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub job_id: Id,
    pub revision: Revision,
    pub last_item_seq: i64,
    pub max_item_seq: i64,
    pub filter: Option<Outcome>,
}

/// A row keeps its sequence even when its content could not be decoded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub item_seq: i64,
    pub item: Option<HistoryItemResult>,
}

/// One item page and its authenticated-by-caller continuation data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page {
    pub revision: Revision,
    pub rows: Vec<Row>,
    pub next: Option<Cursor>,
    /// Number of stored item rows in the fixed operation, including unreadable rows.
    pub recorded: u64,
    /// The terminal sum, absent for summary-only or unreadable operations.
    pub summary: Option<u128>,
    /// A start or item row proves the operation uses item-level recording.
    pub item_history_expected: bool,
}

/// Errors that must not be confused with an unreadable individual row.
#[derive(Debug)]
pub enum Error {
    InvalidRequest,
    VersionChanged,
    NotFound,
    RequiresReadConnection,
    Storage(rusqlite::Error),
}
impl From<rusqlite::Error> for Error {
    fn from(value: rusqlite::Error) -> Self {
        Self::Storage(value)
    }
}

impl AppDb {
    /// Read an ascending item page from a single read-only SQLite snapshot.
    ///
    /// Cursors bind the job, outcome filter, deletion generation, and item bound.
    /// A deleted job is `NotFound`; a changed generation is `VersionChanged`.
    pub fn read_history_items(
        &mut self,
        job_id: &Id,
        filter: Option<Outcome>,
        cursor: Option<Cursor>,
        limit: usize,
    ) -> Result<Page, Error> {
        self.read_history_items_with(job_id, filter, cursor, limit, || {})
    }

    fn read_history_items_with(
        &mut self,
        job_id: &Id,
        filter: Option<Outcome>,
        cursor: Option<Cursor>,
        limit: usize,
        after_anchor: impl FnOnce(),
    ) -> Result<Page, Error> {
        if !(1..=1000).contains(&limit)
            || cursor.as_ref().is_some_and(|c| {
                &c.job_id != job_id
                    || c.filter != filter
                    || c.last_item_seq < 0
                    || c.last_item_seq > c.max_item_seq
                    || c.revision.delete_generation < 0
                    || c.revision.max_record_seq < 0
            })
        {
            return Err(Error::InvalidRequest);
        }
        let key = super::super::id_bytes(job_id).ok_or(Error::InvalidRequest)?;
        if !self.connection.is_readonly("main")? {
            return Err(Error::RequiresReadConnection);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let current = tx.query_row("SELECT delete_generation, last_record_seq FROM job_runtime_history_state WHERE state_key='history'", [], |r| Ok(Revision { delete_generation: r.get(0)?, max_record_seq: r.get(1)? }))?;
        let revision = cursor.as_ref().map_or(current, |c| c.revision);
        if revision.delete_generation != current.delete_generation {
            return Err(Error::VersionChanged);
        }
        if revision.max_record_seq > current.max_record_seq {
            return Err(Error::InvalidRequest);
        }
        let anchor = tx
            .query_row(
                "SELECT link.record_seq, started.job_id IS NOT NULL,
                        record.completed_count, record.overwritten_count, record.failed_count,
                        record.skipped_count, record.unprocessed_count
                 FROM job_runtime_job_link AS link
                 LEFT JOIN job_runtime_job_start AS started USING (job_id)
                 LEFT JOIN job_runtime_job_record AS record USING (job_id)
                 WHERE link.job_id=?1",
                [key.as_slice()],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?, summary_count(r))),
            )
            .optional()?;
        let Some((job_seq, has_start, summary)) = anchor else {
            return Err(Error::NotFound);
        };
        if job_seq <= 0 || job_seq > revision.max_record_seq {
            return Err(Error::InvalidRequest);
        }
        after_anchor();
        let max_item_seq = if let Some(c) = &cursor {
            Some(c.max_item_seq)
        } else {
            tx.query_row(
                "SELECT max(item_seq) FROM job_runtime_item_result WHERE job_id=?1",
                [key.as_slice()],
                |r| r.get::<_, Option<i64>>(0),
            )?
        };
        let max_item_seq = max_item_seq.unwrap_or(-1);
        let recorded: i64 = tx.query_row(
            "SELECT count(*) FROM job_runtime_item_result WHERE job_id=?1 AND item_seq<=?2",
            params![key.as_slice(), max_item_seq],
            |row| row.get(0),
        )?;
        let recorded = u64::try_from(recorded).map_err(|_| Error::InvalidRequest)?;
        let item_history_expected = has_start || recorded > 0;
        let summary = summary.filter(|_| item_history_expected);
        if recorded == 0 {
            tx.commit()?;
            return Ok(Page {
                revision,
                rows: Vec::new(),
                next: None,
                recorded,
                summary,
                item_history_expected,
            });
        }
        let last = cursor.as_ref().map(|c| c.last_item_seq);
        let mut rows = {
            let mut stmt = tx.prepare(include_str!("items_page.sql"))?;
            let results = stmt.query_map(
                params![
                    key.as_slice(),
                    max_item_seq,
                    last,
                    filter.map(Outcome::as_str),
                    (limit + 1) as i64
                ],
                |r| {
                    Ok(Row {
                        item_seq: r.get(0)?,
                        item: read_item(r).ok(),
                    })
                },
            )?;
            results.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let next = if rows.len() > limit {
            rows.pop();
            rows.last().map(|row| Cursor {
                job_id: job_id.clone(),
                revision,
                last_item_seq: row.item_seq,
                max_item_seq,
                filter,
            })
        } else {
            None
        };
        tx.commit()?;
        Ok(Page {
            revision,
            rows,
            next,
            recorded,
            summary,
            item_history_expected,
        })
    }
}

fn summary_count(row: &rusqlite::Row<'_>) -> Option<u128> {
    (2..=6).try_fold(0u128, |sum, index| {
        let count = row.get::<_, i64>(index).ok()?;
        Some(sum + u128::try_from(count).ok()?)
    })
}

fn read_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryItemResult> {
    let raw: String = row.get(2)?;
    let outcome = Outcome::try_from(raw.as_str()).map_err(|()| rusqlite::Error::InvalidQuery)?;
    let size: Option<i64> = row.get(7)?;
    let observed_size = size
        .map(u64::try_from)
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let state: String = row.get(8)?;
    if !matches!(state.as_str(), "exact" | "lower_bound" | "unknown")
        || (observed_size.is_none() != (state == "unknown"))
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let flag = |i| match row.get::<_, i64>(i)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(rusqlite::Error::InvalidQuery),
    };
    let displaced = if flag(9)? {
        Some(HistoryDisplacedItem {
            name: row.get(10)?,
            original_location_display: row.get(11)?,
            trashed_at_ms: row.get(12)?,
            ambiguous: row.get(13)?,
        })
    } else {
        None
    };
    Ok(HistoryItemResult {
        location_display: row.get(1)?,
        outcome,
        reason_key: row.get(3)?,
        review_needed: flag(4)?,
        held: flag(5)?,
        outcome_unknown: flag(6)?,
        observed_size: observed_size.map(ByteSize::from),
        observed_size_state: state,
        displaced,
    })
}

#[cfg(test)]
mod tests;
