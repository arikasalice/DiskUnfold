//! Job registration, cooperative cancellation, and terminal handoff.
mod delivery;
mod model;
mod shutdown;
use delivery::{consume, enqueue};
#[cfg(test)]
use du_core_contracts::job::{Checkpoint, HistoryAppend, JobBody, JobContext};
use du_core_contracts::{
    Id,
    job::{
        HistoryMode, JobControl, JobDescriptor, JobHistory, JobOutcome, JobState, JobSummary,
        ResourceClass,
    },
};
use model::*;
pub use model::{
    CancelResult, DeduplicationKey, JobClock, JobEvent, JobEvents, JobIdentity, JobPriority,
    JobRuntime, RuntimeError, Submission, SubmitResult,
};
pub use shutdown::{ShutdownStatus, ShutdownTicket};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

impl JobRuntime {
    pub fn new(
        identity: Arc<dyn JobIdentity>,
        clock: Arc<dyn JobClock>,
        history: Arc<dyn JobHistory>,
        events: Arc<dyn JobEvents>,
    ) -> Result<Self, RuntimeError> {
        let (sender, receiver) = mpsc::channel();
        let table = Arc::new(Mutex::new(Table {
            sender: Some(sender),
            ..Table::default()
        }));
        let outstanding = Arc::new(AtomicUsize::new(0));
        let consumer_outstanding = outstanding.clone();
        let weak_table = Arc::downgrade(&table);
        let consumer_clock = clock.clone();
        let consumer = std::thread::Builder::new()
            .name("job-delivery".into())
            .spawn(move || {
                consume(
                    receiver,
                    &*history,
                    &*events,
                    &consumer_outstanding,
                    weak_table,
                    consumer_clock,
                )
            })
            .map_err(|_| RuntimeError::ConsumerStart)?;
        Ok(Self {
            table,
            identity,
            clock,
            queue_limit: 64,
            consumer: Mutex::new(Some(consumer)),
            outstanding,
        })
    }

    pub fn register(&self, descriptor: JobDescriptor) -> Result<(), RuntimeError> {
        if descriptor.kind_id.is_empty() || descriptor.version == 0 {
            return Err(RuntimeError::InvalidDescriptor);
        }
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        if table.kinds.contains_key(&descriptor.kind_id) {
            return Err(RuntimeError::DuplicateKind);
        }
        table.kinds.insert(descriptor.kind_id.clone(), descriptor);
        Ok(())
    }
    pub fn submit(&self, submission: Submission) -> Result<SubmitResult, RuntimeError> {
        let now = self.clock.now_ms();
        let body_kind = submission.body.kind_id().to_owned();
        let candidate_id = self.identity.next_id();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        if table.shutting_down {
            return Err(RuntimeError::ShuttingDown);
        }
        expire(&mut table, now);
        let descriptor = table
            .kinds
            .get(&submission.kind_id)
            .cloned()
            .ok_or(RuntimeError::KindUnknown)?;
        if body_kind != submission.kind_id {
            return Err(RuntimeError::InvalidDescriptor);
        }
        if let Some(key) = &submission.deduplication_key
            && let Some((id, _)) = table.jobs.iter().find(|(_, job)| {
                !job.state.is_terminal()
                    && job.submission.kind_id == submission.kind_id
                    && job.submission.deduplication_key.as_ref() == Some(key)
            })
        {
            return Ok(SubmitResult::Existing(id.clone()));
        }
        if table
            .jobs
            .values()
            .filter(|job| job.state == JobState::Queued)
            .count()
            >= self.queue_limit
        {
            return Err(RuntimeError::QueueFull);
        }
        if self.outstanding.load(Ordering::Acquire) >= self.queue_limit {
            return Err(RuntimeError::DeliveryBusy);
        }
        let id = candidate_id;
        if table.jobs.contains_key(&id) {
            return Err(RuntimeError::InvalidState);
        }
        let order = table.next_order;
        table.next_order += 1;
        let Submission {
            kind_id,
            deduplication_key,
            body,
            priority,
            scope_summary,
            correlation_id,
            sample,
        } = submission;
        self.outstanding.fetch_add(1, Ordering::AcqRel);
        table.jobs.insert(
            id.clone(),
            Entry {
                descriptor,
                submission: StoredSubmission {
                    kind_id,
                    deduplication_key,
                    priority,
                    scope_summary,
                    correlation_id,
                    sample,
                },
                body: Some(body),
                state: JobState::Queued,
                sequence: 1,
                cancel: Arc::new(Cancellation(AtomicBool::new(false))),
                started_at_ms: None,
                summary: None,
                order,
            },
        );
        Ok(SubmitResult::Created(id))
    }
    pub fn state(&self, id: &Id) -> Result<JobState, RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        expire(&mut table, now);
        table
            .jobs
            .get(id)
            .map(|job| job.state)
            .ok_or(RuntimeError::NotFound)
    }
    pub fn cancel(&self, id: &Id) -> Result<CancelResult, RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        expire(&mut table, now);
        let job = table.jobs.get_mut(id).ok_or(RuntimeError::NotFound)?;
        if job.state.is_terminal() {
            return Ok(CancelResult::AlreadyTerminal(job.state));
        }
        if !job.descriptor.controls.contains(&JobControl::Cancel) {
            return Ok(CancelResult::Unsupported(job.state));
        }
        if job.state == JobState::CancelRequested {
            return Ok(CancelResult::NoEffect(job.state));
        }
        job.cancel.0.store(true, Ordering::Release);
        if job.state == JobState::Queued {
            job.state = JobState::CancelRequested;
            drop(table);
            self.finish(
                id,
                JobOutcome {
                    counts: Default::default(),
                    completed_scope: false,
                    failure_reason_key: None,
                    unprocessed_summary: None,
                    result_version_id: None,
                },
            )?;
            return Ok(CancelResult::Accepted(JobState::Cancelled));
        }
        job.state = JobState::CancelRequested;
        job.sequence += 1;
        let event = JobEvent::StateChanged {
            job_id: id.clone(),
            state: job.state,
            sequence: job.sequence,
        };
        enqueue(&table, Delivery::Event(event))?;
        drop(table);
        Ok(CancelResult::Accepted(JobState::CancelRequested))
    }
    /// Runs the oldest eligible queued job on the calling worker thread.
    pub fn run_next(&self) -> Result<Option<Id>, RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        let active = table
            .jobs
            .values()
            .filter(|job| {
                matches!(
                    job.state,
                    JobState::Running
                        | JobState::PauseRequested
                        | JobState::Paused
                        | JobState::CancelRequested
                )
            })
            .count();
        let changing = table.jobs.values().any(|job| {
            matches!(
                job.state,
                JobState::Running
                    | JobState::PauseRequested
                    | JobState::Paused
                    | JobState::CancelRequested
            ) && job
                .descriptor
                .resource_classes
                .contains(&ResourceClass::FileChange)
        });
        if active >= 4 {
            return Ok(None);
        }
        let selected = table
            .jobs
            .iter()
            .filter(|(_, job)| {
                job.state == JobState::Queued
                    && !(changing
                        && job
                            .descriptor
                            .resource_classes
                            .contains(&ResourceClass::FileChange))
            })
            .min_by_key(|(_, job)| (job.submission.priority, job.order))
            .map(|(id, _)| id.clone());
        let Some(id) = selected else {
            return Ok(None);
        };
        let job = table.jobs.get(&id).ok_or(RuntimeError::NotFound)?;
        if job.body.is_none() {
            return Err(RuntimeError::InvalidState);
        }
        let sequence = job.sequence + 1;
        let event = JobEvent::StateChanged {
            job_id: id.clone(),
            state: JobState::Running,
            sequence,
        };
        enqueue(&table, Delivery::Event(event))?;
        let job = table.jobs.get_mut(&id).ok_or(RuntimeError::NotFound)?;
        job.state = JobState::Running;
        job.started_at_ms = Some(now);
        job.sequence = sequence;
        let context = job.cancel.clone();
        let body = job.body.take().ok_or(RuntimeError::InvalidState)?;
        drop(table);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body.run(context)))
            .unwrap_or_else(|_| JobOutcome {
                counts: Default::default(),
                completed_scope: false,
                failure_reason_key: Some("support.internal_error".into()),
                unprocessed_summary: None,
                result_version_id: None,
            });
        drop(body);
        self.finish(&id, outcome)?;
        Ok(Some(id))
    }
    fn finish(&self, id: &Id, outcome: JobOutcome) -> Result<(), RuntimeError> {
        let finished_at_ms = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        let job = table.jobs.get_mut(id).ok_or(RuntimeError::NotFound)?;
        if job.state.is_terminal() {
            return Ok(());
        }
        let state = final_state(job, &outcome);
        job.state = state;
        job.sequence += 1;
        let summary = JobSummary {
            job_id: id.clone(),
            kind_id: job.descriptor.kind_id.clone(),
            kind_version: job.descriptor.version,
            scope_summary: job.submission.scope_summary.clone(),
            state,
            counts: outcome.counts,
            failure_reason_key: outcome.failure_reason_key,
            unprocessed_summary: outcome.unprocessed_summary,
            result_version_id: outcome.result_version_id,
            started_at_ms: job.started_at_ms,
            finished_at_ms,
            correlation_id: job.submission.correlation_id.clone(),
        };
        let released_body = job.body.take();
        job.submission.deduplication_key = None;
        job.summary = Some(summary.clone());
        let record = job.descriptor.history != HistoryMode::None && !job.submission.sample;
        let sequence = job.sequence;
        let delivery = enqueue(
            &table,
            Delivery::Terminal {
                summary: Box::new(summary),
                sequence,
                record,
            },
        );
        drop(table);
        drop(released_body);
        delivery
    }
    #[cfg(test)]
    fn flush(&self) -> Result<(), RuntimeError> {
        let (sender, receiver) = mpsc::channel();
        let table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        enqueue(&table, Delivery::Fence(sender))?;
        drop(table);
        receiver
            .recv()
            .map_err(|_| RuntimeError::DeliveryUnavailable)
    }
}

fn final_state(job: &Entry, outcome: &JobOutcome) -> JobState {
    if job.cancel.0.load(Ordering::Acquire) {
        return JobState::Cancelled;
    }
    if outcome.completed_scope {
        if job
            .descriptor
            .resource_classes
            .contains(&ResourceClass::FileChange)
            && outcome.counts.completed > 0
            && (outcome.counts.failed > 0 || outcome.counts.skipped > 0)
        {
            return JobState::Partial;
        }
        return JobState::Done;
    }
    if outcome.counts.completed > 0 || outcome.counts.overwritten > 0 {
        JobState::Partial
    } else {
        JobState::Failed
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RuntimeError {}

#[cfg(test)]
mod tests;
