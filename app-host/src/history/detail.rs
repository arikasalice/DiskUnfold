//! Adapt one SQLite detail snapshot to the engine's history reader.

use super::{HistoryReader, db, history_id, project_summary};
use db::detail::{Detail, Link, Part, Successor};
use du_core_contracts::{
    Id,
    history_ipc::{HistoryEndState, HistoryFollowUp, HistoryRelation},
};
use du_engine_api::history::detail::{Read, StorageUnavailable, StoredDetail};

impl Read for HistoryReader {
    fn get(&self, job: &Id) -> Result<Option<StoredDetail>, StorageUnavailable> {
        let detail = self
            .connection
            .lock()
            .map_err(|_| StorageUnavailable)?
            .read_history_detail(job)
            .map_err(|_| StorageUnavailable)?;
        Ok(detail.map(project))
    }
}

fn project(detail: Detail) -> StoredDetail {
    let mut missing = Missing::default();
    let linked = linked(detail.link, detail.predecessor_present, &mut missing);
    let summary = summary(
        detail.summary,
        detail.start,
        detail.interrupted_at_ms,
        &mut missing,
    )
    .map(|mut row| {
        if row.scope_summary.is_none() {
            missing.add("read_failed");
        }
        row.relation = linked.relation;
        project_summary(row, linked.predecessor)
    });
    let follow_ups = missing.read(detail.successors).map(|rows| {
        rows.into_iter()
            .map(|row| follow_up(row, &mut missing))
            .collect()
    });
    StoredDetail {
        summary,
        executed_plan: linked.executed_plan,
        requires_plan_read: linked.requires_plan_read,
        follow_ups,
        missing_reasons: missing.0,
    }
}

fn summary(
    terminal: Part<db::Summary>,
    start: Part<db::detail::Start>,
    interrupted_at: Part<i64>,
    missing: &mut Missing,
) -> Option<db::Summary> {
    if terminal != Part::Absent {
        return missing.read(terminal);
    }
    match (start, interrupted_at) {
        (Part::Present(start), Part::Present(_)) => {
            missing.add("interrupted");
            Some(db::Summary {
                kind_id: start.kind_id,
                kind_version: start.kind_version,
                state: HistoryEndState::Interrupted,
                scope_summary: start.scope_summary,
                counts: None,
                started_at_ms: Some(start.started_at_ms),
                finished_at_ms: None,
                predecessor_job_id: None,
                relation: None,
            })
        }
        _ => {
            missing.add("read_failed");
            None
        }
    }
}

struct Linked {
    predecessor: Option<Id>,
    relation: Option<HistoryRelation>,
    executed_plan: Option<Id>,
    requires_plan_read: bool,
}

fn linked(link: Part<Link>, predecessor_present: Part<bool>, missing: &mut Missing) -> Linked {
    let Some(link) = missing.read(link) else {
        return Linked {
            predecessor: None,
            relation: None,
            executed_plan: None,
            requires_plan_read: true,
        };
    };
    let requires_plan_read =
        link.plan_version_id != Part::Absent || link.plan_lineage_id != Part::Absent;
    let mut predecessor = missing.reference(link.predecessor_job_id);
    if predecessor.is_some() && predecessor_present != Part::Present(true) {
        missing.add("read_failed");
        predecessor = None;
    }
    Linked {
        predecessor,
        relation: missing.read(link.relation),
        executed_plan: missing.reference(link.plan_version_id),
        requires_plan_read,
    }
}

fn follow_up(row: Successor, missing: &mut Missing) -> HistoryFollowUp {
    let end = missing.read(row.end);
    let relation = missing.read(row.relation);
    if end.is_none() || relation.is_none() {
        missing.add("read_failed");
    }
    HistoryFollowUp {
        job_ref: missing.reference(row.job_id),
        relation,
        state: end.as_ref().map(|end| end.state),
        finished_at_ms: end.and_then(|end| end.finished_at_ms),
    }
}

#[derive(Default)]
struct Missing(Vec<String>);

impl Missing {
    fn add(&mut self, reason: &str) {
        if !self.0.iter().any(|known| known == reason) {
            self.0.push(reason.to_owned());
        }
    }

    fn read<T>(&mut self, part: Part<T>) -> Option<T> {
        match part {
            Part::Present(value) => Some(value),
            Part::Absent => None,
            Part::Unavailable => {
                self.add("read_failed");
                None
            }
        }
    }

    fn reference(&mut self, part: Part<[u8; 16]>) -> Option<Id> {
        let id = history_id(self.read(part)?);
        if id.is_none() {
            self.add("read_failed");
        }
        id
    }
}
