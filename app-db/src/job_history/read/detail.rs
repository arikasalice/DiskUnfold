//! Snapshot-consistent, component-wise reads for a single history record.

use du_core_contracts::Id;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::{EndState, Error, Relation, Summary, read_summary};
use crate::AppDb;

/// Distinguishes data that was not stored from data that could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Part<T> {
    /// A stored value was read successfully.
    Present(T),
    /// No corresponding row was stored.
    Absent,
    /// A row or component exists but could not be decoded or queried.
    Unavailable,
}

/// The safe, path-free fields of a stored start record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Start {
    /// Registered kind identifier as stored.
    pub kind_id: String,
    /// Version of the registered kind.
    pub kind_version: u32,
    /// Validated path-free scope, when recognized.
    pub scope_summary: Option<String>,
    /// Start time in Unix milliseconds.
    pub started_at_ms: i64,
}

/// The predecessor and relation recorded on a history link.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Link {
    /// The stored predecessor reference, if any.
    pub predecessor_job_id: Part<[u8; 16]>,
    /// The recorded relation, if any.
    pub relation: Part<Relation>,
    /// The recorded execution plan, when the operation used one.
    pub plan_version_id: Part<[u8; 16]>,
    /// The lineage recorded alongside the execution plan.
    pub plan_lineage_id: Part<[u8; 16]>,
}

/// The terminal facts needed to list a follow-up without opening its detail.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuccessorEnd {
    pub state: EndState,
    pub finished_at_ms: Option<i64>,
}

/// A persisted successor link, including an independently decoded relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Successor {
    /// The stored successor reference, or an unreadable reference.
    pub job_id: Part<[u8; 16]>,
    /// The stored relationship to the predecessor.
    pub relation: Part<Relation>,
    /// The stored end state and time, if readable.
    pub end: Part<SuccessorEnd>,
}

/// Independently readable components from the same SQLite snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Detail {
    /// Terminal record, when stored and readable.
    pub summary: Part<Summary>,
    /// Start record, when stored and readable.
    pub start: Part<Start>,
    /// Time an interrupted job was detected, when recorded.
    pub interrupted_at_ms: Part<i64>,
    /// The job's own persisted link.
    pub link: Part<Link>,
    /// Whether the referenced predecessor has a stored history link.
    /// Absent means that there is no predecessor reference to resolve.
    pub predecessor_present: Part<bool>,
    /// Persisted links naming this job as predecessor; an empty vector is complete.
    pub successors: Part<Vec<Successor>>,
}

impl AppDb {
    /// Read all available components for a job using one read-only snapshot.
    ///
    /// Returns `None` only if no record, start, interruption or link exists.
    /// Component failures remain visible without discarding successfully read data.
    pub fn read_history_detail(&mut self, job_id: &Id) -> Result<Option<Detail>, Error> {
        let bytes = super::super::id_bytes(job_id).ok_or(Error::InvalidRequest)?;
        self.read_history_detail_with(bytes, || {})
    }

    fn read_history_detail_with(
        &mut self,
        job_id: [u8; 16],
        after_anchor: impl FnOnce(),
    ) -> Result<Option<Detail>, Error> {
        if !self.connection.is_readonly("main")? {
            return Err(Error::RequiresReadConnection);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM job_runtime_job_record WHERE job_id=?1
                 UNION ALL SELECT 1 FROM job_runtime_job_start WHERE job_id=?1
                 UNION ALL SELECT 1 FROM job_runtime_job_interrupt WHERE job_id=?1
                 UNION ALL SELECT 1 FROM job_runtime_job_link WHERE job_id=?1)",
            [job_id.as_slice()],
            |row| row.get(0),
        )?;
        if !exists {
            tx.commit()?;
            return Ok(None);
        }
        after_anchor();
        let link = link(&tx, &job_id);
        let predecessor_present = match &link {
            Part::Present(Link {
                predecessor_job_id: Part::Present(id),
                ..
            }) => part(
                tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM job_runtime_job_link WHERE job_id=?1)",
                    [id.as_slice()],
                    |row| row.get(0),
                )
                .map(Some),
            ),
            Part::Unavailable
            | Part::Present(Link {
                predecessor_job_id: Part::Unavailable,
                ..
            }) => Part::Unavailable,
            _ => Part::Absent,
        };
        let result = Detail {
            summary: summary(&tx, &job_id),
            start: start(&tx, &job_id),
            interrupted_at_ms: part(
                tx.query_row(
                    "SELECT detected_at_ms FROM job_runtime_job_interrupt WHERE job_id=?1",
                    [job_id.as_slice()],
                    |row| row.get(0),
                )
                .optional(),
            ),
            link,
            predecessor_present,
            successors: successors(&tx, &job_id),
        };
        tx.commit()?;
        Ok(Some(result))
    }
}

fn part<T>(result: rusqlite::Result<Option<T>>) -> Part<T> {
    match result {
        Ok(Some(value)) => Part::Present(value),
        Ok(None) => Part::Absent,
        Err(_) => Part::Unavailable,
    }
}

fn summary(tx: &Transaction<'_>, id: &[u8; 16]) -> Part<Summary> {
    part(
        tx.query_row(
            "SELECT job_id, 0, 0, kind_id, kind_version, final_state, scope_summary,
                completed_count, overwritten_count, failed_count, skipped_count, unprocessed_count,
                started_at_ms, finished_at_ms, NULL, NULL
         FROM job_runtime_job_record WHERE job_id=?1",
            [id.as_slice()],
            read_summary,
        )
        .optional(),
    )
}

fn start(tx: &Transaction<'_>, id: &[u8; 16]) -> Part<Start> {
    part(
        tx.query_row(
            "SELECT kind_id, kind_version, scope_summary, started_at_ms
         FROM job_runtime_job_start WHERE job_id=?1",
            [id.as_slice()],
            |row| {
                let kind_id: String = row.get(0)?;
                let kind_version: u32 = row.get(1)?;
                let scope: String = row.get(2)?;
                let scope_summary = (kind_id == "scan.metadata"
                    && kind_version == 1
                    && super::super::valid_scope(&scope))
                .then_some(scope);
                Ok(Start {
                    kind_id,
                    kind_version,
                    scope_summary,
                    started_at_ms: row.get(3)?,
                })
            },
        )
        .optional(),
    )
}

fn link(tx: &Transaction<'_>, id: &[u8; 16]) -> Part<Link> {
    part(
        tx.query_row(
            "SELECT predecessor_job_id, relation_kind, plan_version_id, plan_lineage_id
             FROM job_runtime_job_link WHERE job_id=?1",
            [id.as_slice()],
            |row| {
                Ok(Link {
                    predecessor_job_id: part(row.get(0)),
                    relation: relation(row.get(1)),
                    plan_version_id: part(row.get(2)),
                    plan_lineage_id: part(row.get(3)),
                })
            },
        )
        .optional(),
    )
}

fn successors(tx: &Transaction<'_>, id: &[u8; 16]) -> Part<Vec<Successor>> {
    let result = (|| {
        let mut statement = tx.prepare(
            "SELECT link.job_id, link.relation_kind,
                COALESCE(record.final_state,
                         CASE WHEN interrupted.job_id IS NOT NULL THEN 'interrupted' END),
                record.finished_at_ms
             FROM job_runtime_job_link AS link
             LEFT JOIN job_runtime_job_record AS record USING (job_id)
             LEFT JOIN job_runtime_job_interrupt AS interrupted USING (job_id)
             WHERE link.predecessor_job_id=?1 ORDER BY link.record_seq, link.job_id",
        )?;
        let rows = statement.query_map(params![id.as_slice()], |row| {
            Ok(Successor {
                job_id: row.get(0).map_or(Part::Unavailable, Part::Present),
                relation: relation(row.get(1)),
                end: part(successor_end(row)),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })();
    match result {
        Ok(rows) => Part::Present(rows),
        Err(_) => Part::Unavailable,
    }
}

fn relation(value: rusqlite::Result<Option<String>>) -> Part<Relation> {
    part(value.and_then(|value| {
        value
            .map(|value| {
                Relation::try_from(value.as_str()).map_err(|()| rusqlite::Error::InvalidQuery)
            })
            .transpose()
    }))
}

fn successor_end(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<SuccessorEnd>> {
    let Some(state) = row.get::<_, Option<String>>(2)? else {
        return Ok(None);
    };
    Ok(Some(SuccessorEnd {
        state: EndState::try_from(state.as_str()).map_err(|()| rusqlite::Error::InvalidQuery)?,
        finished_at_ms: row.get(3)?,
    }))
}

#[cfg(test)]
mod tests;
