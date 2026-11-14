//! Batched job progress (F24 FR2.HD1.app-host.DD1 and FR14.HD1.app-host.DD1).
//!
//! A job body overwrites one lock-free cell per job. At each batch tick the
//! delivery thread reads the cells under the job-table lock, assigns the next
//! job sequence to each changed value, and queues it behind earlier job events,
//! so progress shares the job sequence space and delivery order.
use crate::listing::{JobWaitReason, wait_reason};
use crate::model::{Delivery, Entry, JobClock, ProgressCell, ProgressSlot, Table};
use du_core_contracts::{
    Id,
    job::{JobProgress, ProgressUnit},
};
use std::{
    sync::{Arc, Mutex, Weak, atomic::Ordering},
    time::Duration,
};

/// The progress batch interval.
const INTERVAL: Duration = Duration::from_millis(100);
/// Marks an unknown total; confirmed totals are clamped below it.
const UNKNOWN_TOTAL: u64 = u64::MAX;

/// The latest normalized progress of one job (F24 IF-14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobProgressEvent {
    /// The job whose progress changed.
    pub job_id: Id,
    /// Per-job sequence shared with state changes and the terminal event.
    pub sequence: u64,
    /// The unit declared by the job kind's descriptor.
    pub unit: ProgressUnit,
    /// Units processed so far.
    pub completed: u64,
    /// The confirmed total, or none while the total is not established.
    pub total: Option<u64>,
    /// The confirmed total less the completed amount, saturating at zero, or
    /// none while the total is not established.
    pub remaining: Option<u64>,
    /// The wait reason supported by the job state at the tick.
    pub wait_reason: JobWaitReason,
    /// UTC milliseconds at which the runtime read this value.
    pub observed_at_ms: i64,
}

impl ProgressCell {
    /// Overwrites the latest value, or discards it after the job has ended.
    pub(crate) fn report(&self, job: &Id, progress: JobProgress) {
        if self.closed.load(Ordering::Acquire) {
            if !self.late_logged.swap(true, Ordering::AcqRel) {
                eprintln!(
                    "DiskUnfold job-runtime: discarded progress reported after job {} ended",
                    job.as_str()
                );
            }
            return;
        }
        let total = progress
            .total
            .map_or(UNKNOWN_TOTAL, |total| total.min(UNKNOWN_TOTAL - 1));
        // A tick racing a report may pair amounts from adjacent reports; the
        // next tick reads the settled value, so the latest value is not lost.
        self.completed.store(progress.completed, Ordering::Release);
        self.total.store(total, Ordering::Release);
        self.reported.store(true, Ordering::Release);
    }

    fn read(&self) -> Option<JobProgress> {
        if !self.reported.load(Ordering::Acquire) {
            return None;
        }
        let completed = self.completed.load(Ordering::Acquire);
        let total = self.total.load(Ordering::Acquire);
        Some(JobProgress {
            completed,
            total: (total != UNKNOWN_TOTAL).then_some(total),
        })
    }

    /// Permits the next value once the previous one has been delivered.
    pub(crate) fn delivered(&self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

impl ProgressSlot {
    /// Creates the cell handed to the starting body.
    pub(crate) fn start(&mut self) -> Arc<ProgressCell> {
        let cell = Arc::new(ProgressCell::default());
        self.cell = Some(cell.clone());
        cell
    }

    /// Discards later reports; called when the job reaches its terminal state.
    pub(crate) fn close(&self) {
        if let Some(cell) = &self.cell {
            cell.closed.store(true, Ordering::Release);
        }
    }
}

/// Queues each job's changed value at most once per interval.
///
/// The caller holds the job-table lock, so a progress sequence is assigned and
/// queued in the same order as state changes and terminal results. Without a
/// monotonic time every call is a tick; the caller then paces the calls.
pub(crate) fn tick(table: &mut Table, now: Option<Duration>, observed_at_ms: i64) {
    if let (Some(now), Some(due)) = (now, table.progress_due)
        && now < due
    {
        return;
    }
    table.progress_due = now.map(|now| now.saturating_add(INTERVAL));
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
            .send(Delivery::Progress(Box::new(event), cell.clone()))
            .is_err()
        {
            cell.in_flight.store(false, Ordering::Release);
            return;
        }
        job.sequence = sequence;
        job.progress.published = Some(value);
    }
}

fn changed(
    id: &Id,
    job: &Entry,
    observed_at_ms: i64,
) -> Option<(JobProgressEvent, Arc<ProgressCell>, JobProgress)> {
    // A terminal job's last sequence belongs to job.finished (FR14).
    if job.state.is_terminal() {
        return None;
    }
    let cell = job.progress.cell.as_ref()?;
    // At most one value per job waits in the delivery queue.
    if cell.in_flight.load(Ordering::Acquire) {
        return None;
    }
    let value = cell.read()?;
    if job.progress.published == Some(value) {
        return None;
    }
    let event = JobProgressEvent {
        job_id: id.clone(),
        sequence: job.sequence.checked_add(1)?,
        unit: job.descriptor.progress_unit,
        completed: value.completed,
        total: value.total,
        remaining: value
            .total
            .map(|total| total.saturating_sub(value.completed)),
        wait_reason: wait_reason(job.state),
        observed_at_ms,
    };
    Some((event, cell.clone(), value))
}

/// Paces progress ticks on the delivery thread and avoids taking the job-table
/// lock before a tick is due.
#[derive(Default)]
pub(crate) struct ProgressTicker {
    next: Option<Duration>,
}

impl ProgressTicker {
    /// The longest wait for a delivery before the next tick is due.
    pub(crate) fn wait(&self, now: Option<Duration>) -> Duration {
        match (self.next, now) {
            (Some(next), Some(now)) => next.saturating_sub(now).min(INTERVAL),
            (None, Some(_)) => Duration::ZERO,
            (_, None) => INTERVAL,
        }
    }

    /// Runs a tick when one is due at the injected monotonic time.
    pub(crate) fn poll(&mut self, table: &Weak<Mutex<Table>>, clock: &dyn JobClock) {
        let now = clock.monotonic_now();
        if let (Some(next), Some(now)) = (self.next, now)
            && now < next
        {
            return;
        }
        let observed_at_ms = clock.now_ms();
        let due = table.upgrade().and_then(|table| {
            let mut table = table.lock().ok()?;
            tick(&mut table, now, observed_at_ms);
            table.progress_due
        });
        self.next = due.or_else(|| now.map(|now| now.saturating_add(INTERVAL)));
    }
}

#[cfg(test)]
impl crate::JobRuntime {
    /// Runs one progress tick on the caller's thread at the injected clock time.
    pub(crate) fn tick_progress(&self) -> Result<(), crate::RuntimeError> {
        let now = self.clock.monotonic_now();
        let observed_at_ms = self.clock.now_ms();
        let mut table = self
            .table
            .lock()
            .map_err(|_| crate::RuntimeError::Poisoned)?;
        tick(&mut table, now, observed_at_ms);
        Ok(())
    }

    /// Returns whether a late report for the job was discarded and logged.
    pub(crate) fn late_progress_logged(&self, id: &Id) -> Result<bool, crate::RuntimeError> {
        let table = self
            .table
            .lock()
            .map_err(|_| crate::RuntimeError::Poisoned)?;
        let job = table.jobs.get(id).ok_or(crate::RuntimeError::NotFound)?;
        Ok(job
            .progress
            .cell
            .as_ref()
            .is_some_and(|cell| cell.late_logged.load(Ordering::Acquire)))
    }
}
