//! Batched per-lane scan status reported by a job body (F02 FR10.HD1.app-host.DD1).
use crate::model::{Delivery, Entry, LaneStatusCell, LaneStatusSlot, Table};
use du_core_contracts::{Id, job::LaneStatusReport};
use std::sync::{Arc, atomic::Ordering};

/// The latest per-lane scan status of one job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobLaneStatusEvent {
    /// The job whose lane status changed.
    pub job_id: Id,
    /// Per-job sequence shared with progress, state changes, and the terminal event.
    pub sequence: u64,
    /// UTC milliseconds at which the runtime read this value.
    pub observed_at_ms: i64,
    /// The latest lane values reported by the job body.
    pub status: LaneStatusReport,
}

impl LaneStatusCell {
    /// Replaces the latest value without acquiring the job table lock.
    pub(crate) fn report(&self, job: &Id, status: LaneStatusReport) {
        let status = Arc::new(status);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            if !state.late_logged {
                state.late_logged = true;
                drop(state);
                eprintln!(
                    "DiskUnfold job-runtime: discarded lane status reported after job {} ended",
                    job.as_str()
                );
            }
            return;
        }
        state.latest = Some(status);
    }

    fn read(&self) -> Option<Arc<LaneStatusReport>> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .latest
            .clone()
    }
}

impl LaneStatusSlot {
    /// Discards later reports; called when the job reaches its terminal state.
    pub(crate) fn close(&self) {
        if let Some(cell) = &self.cell {
            cell.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .closed = true;
        }
    }
}

/// Queues changed lane status after progress at the same 100 ms batch tick.
///
/// The caller holds the job-table lock, so each sequence is assigned and
/// queued in order with progress, state changes, and terminal results.
pub(crate) fn tick(table: &mut Table, observed_at_ms: i64) {
    let Table { jobs, sender, .. } = table;
    let Some(sender) = sender.as_ref() else {
        return;
    };
    for (id, job) in jobs.iter_mut() {
        let Some((event, cell, value)) = changed(id, job, observed_at_ms) else {
            continue;
        };
        let sequence = event.sequence;
        cell.in_flight.store(true, Ordering::Release);
        if sender
            .send(Delivery::LaneStatus(Box::new(event), cell.clone()))
            .is_err()
        {
            cell.in_flight.store(false, Ordering::Release);
            return;
        }
        job.sequence = sequence;
        job.progress.lane_status.published = Some(value);
    }
}

fn changed(
    id: &Id,
    job: &Entry,
    observed_at_ms: i64,
) -> Option<(JobLaneStatusEvent, Arc<LaneStatusCell>, LaneStatusReport)> {
    if job.state.is_terminal() {
        return None;
    }
    let cell = job.progress.lane_status.cell.as_ref()?;
    if cell.in_flight.load(Ordering::Acquire) {
        return None;
    }
    let latest = cell.read()?;
    if job.progress.lane_status.published.as_ref() == Some(latest.as_ref()) {
        return None;
    }
    let value = latest.as_ref().clone();
    let event = JobLaneStatusEvent {
        job_id: id.clone(),
        sequence: job.sequence.checked_add(1)?,
        observed_at_ms,
        status: value.clone(),
    };
    Some((event, cell.clone(), value))
}
