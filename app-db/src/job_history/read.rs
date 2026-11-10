//! Read a bounded job-history page from a dedicated SQLite read connection.

use rusqlite::{TransactionBehavior, params};

use crate::AppDb;
pub use du_core_contracts::history_ipc::{
    HistoryEndState as EndState, HistoryListFilter as Filter, HistoryRelation as Relation,
};

/// The insertion bound and deletion generation captured on the first page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Revision {
    pub delete_generation: i64,
    pub max_record_seq: i64,
}

/// The last returned row's stable sort key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position {
    pub reckon_at_ms: i64,
    pub record_seq: i64,
}

/// Internal page position and original filter. Transport code authenticates it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Continuation {
    pub revision: Revision,
    pub position: Position,
    pub filter: Filter,
}

/// The safe summary fields needed by history list consumers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Summary {
    pub kind_id: String,
    pub kind_version: u32,
    pub state: EndState,
    /// Only a registered, validated path-free summary is returned.
    pub scope_summary: Option<String>,
    pub counts: Option<du_core_contracts::job::JobCounts>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub predecessor_job_id: Option<[u8; 16]>,
    pub relation: Option<Relation>,
}

/// A linked row. A missing or unreadable summary remains visible as an omission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub job_id: [u8; 16],
    pub position: Position,
    pub summary: Option<Summary>,
}

/// One stable page and its internal continuation, if any.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page {
    pub revision: Revision,
    pub rows: Vec<Entry>,
    pub next: Option<Continuation>,
}

/// A page could not be fulfilled without changing its meaning.
#[derive(Debug)]
pub enum Error {
    InvalidRequest,
    VersionChanged,
    RequiresReadConnection,
    Storage(rusqlite::Error),
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

impl AppDb {
    /// Reads a page from one SQLite snapshot on an already opened read-only connection.
    ///
    /// New records after the first page are fenced by the saved sequence. A
    /// change to the deletion generation invalidates subsequent pages.
    pub fn read_history_page(
        &mut self,
        filter: Filter,
        continuation: Option<Continuation>,
        limit: usize,
    ) -> Result<Page, Error> {
        self.read_history_page_with(filter, continuation, limit, || {})
    }

    fn read_history_page_with(
        &mut self,
        filter: Filter,
        continuation: Option<Continuation>,
        limit: usize,
        after_state: impl FnOnce(),
    ) -> Result<Page, Error> {
        if !(1..=1000).contains(&limit)
            || matches!((filter.finished_from_ms, filter.finished_before_ms), (Some(a), Some(b)) if a >= b)
            || continuation.as_ref().is_some_and(|cursor| {
                cursor.filter != filter
                    || cursor.position.record_seq <= 0
                    || cursor.position.record_seq > cursor.revision.max_record_seq
                    || cursor.revision.delete_generation < 0
            })
        {
            return Err(Error::InvalidRequest);
        }
        if !self.connection.is_readonly("main")? {
            return Err(Error::RequiresReadConnection);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let current = tx.query_row(
            "SELECT delete_generation, last_record_seq
             FROM job_runtime_history_state WHERE state_key = 'history'",
            [],
            |row| {
                Ok(Revision {
                    delete_generation: row.get(0)?,
                    max_record_seq: row.get(1)?,
                })
            },
        )?;
        let revision = continuation
            .as_ref()
            .map_or(current, |cursor| cursor.revision);
        if revision.delete_generation != current.delete_generation {
            return Err(Error::VersionChanged);
        }
        if revision.max_record_seq > current.max_record_seq {
            return Err(Error::InvalidRequest);
        }
        after_state();
        let position = continuation.map(|cursor| cursor.position);
        let categories = serde_json::to_string(&filter).map_err(|_| Error::InvalidRequest)?;
        let mut result = {
            let mut statement = tx.prepare(include_str!("read_page.sql"))?;
            statement
                .query_map(
                    params![
                        revision.max_record_seq,
                        filter.finished_from_ms,
                        filter.finished_before_ms,
                        position.map(|key| key.reckon_at_ms),
                        position.map(|key| key.record_seq),
                        categories,
                        (limit + 1) as i64,
                    ],
                    |row| {
                        Ok(Entry {
                            job_id: row.get(0)?,
                            position: Position {
                                reckon_at_ms: row.get(1)?,
                                record_seq: row.get(2)?,
                            },
                            summary: read_summary(row).ok(),
                        })
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let next = if result.len() > limit {
            result.pop();
            result.last().map(|row| Continuation {
                revision,
                position: row.position,
                filter,
            })
        } else {
            None
        };
        tx.commit()?;
        Ok(Page {
            revision,
            rows: result,
            next,
        })
    }
}

fn read_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<Summary> {
    use du_core_contracts::job::JobCounts;

    let kind_id: String = row.get(3)?;
    let kind_version = row.get(4)?;
    let state: String = row.get(5)?;
    let state = EndState::try_from(state.as_str()).map_err(|()| rusqlite::Error::InvalidQuery)?;
    let scope: Option<String> = row.get(6)?;
    let counts = if state == EndState::Interrupted {
        None
    } else {
        let count = |index| {
            let value: i64 = row.get(index)?;
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        };
        Some(JobCounts {
            completed: count(7)?,
            overwritten: count(8)?,
            failed: count(9)?,
            skipped: count(10)?,
            unprocessed: count(11)?,
        })
    };
    let relation: Option<String> = row.get(15)?;
    let relation = relation
        .as_deref()
        .map(Relation::try_from)
        .transpose()
        .map_err(|()| rusqlite::Error::InvalidQuery)?;
    Ok(Summary {
        scope_summary: scope.filter(|scope| {
            kind_id == "scan.metadata" && kind_version == 1 && super::valid_scope(scope)
        }),
        kind_id,
        kind_version,
        state,
        counts,
        started_at_ms: row.get(12)?,
        finished_at_ms: row.get(13)?,
        predecessor_job_id: row.get(14)?,
        relation,
    })
}

#[cfg(test)]
mod tests;
