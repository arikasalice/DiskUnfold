//! Job registration, cooperative cancellation, and terminal handoff.
mod control;
mod delivery;
mod flows;
mod history_recovery;
mod model;
mod publication;
mod result_publisher;
mod shutdown;
use control::final_state;
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
pub use history_recovery::recover_history;
pub use model::FlowLimits;
use model::*;
pub use model::{
    CancelResult, DeduplicationKey, JobClock, JobEvent, JobEvents, JobIdentity, JobPriority,
    JobRuntime, JobSnapshot, ResultBinding, RuntimeError, Submission, SubmitResult,
};
pub use publication::DuplicateTerminal;
pub use shutdown::{ShutdownStatus, ShutdownTicket};
use std::sync::{
    Arc, Condvar, Mutex,
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
        Self::new_with_limits(identity, clock, history, events, FlowLimits::disabled())
    }
    pub fn new_with_limits(
        identity: Arc<dyn JobIdentity>,
        clock: Arc<dyn JobClock>,
        history: Arc<dyn JobHistory>,
        events: Arc<dyn JobEvents>,
        limits: FlowLimits,
    ) -> Result<Self, RuntimeError> {
        Self::new_with_content_io_limit(identity, clock, history, events, limits, 0)
    }

    /// Creates a runtime with an explicitly supplied session-wide content-read flow limit.
    /// A zero limit disables content-read flows. Existing constructors leave them disabled.
    /// Jobs declaring both metadata and content classes retain metadata-flow selection;
    /// the current job context does not select a class for an individual flow.
    pub fn new_with_content_io_limit(
        identity: Arc<dyn JobIdentity>,
        clock: Arc<dyn JobClock>,
        history: Arc<dyn JobHistory>,
        events: Arc<dyn JobEvents>,
        limits: FlowLimits,
        content_limit: usize,
    ) -> Result<Self, RuntimeError> {
        let flow_limits = Arc::new(Limits::new(limits, content_limit));
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
            flow_limits,
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
        self.submit_inner(submission, None, None, |_| Ok(()))
    }

    /// Submits a job with a result binding stored for the job entry lifetime.
    pub fn submit_with_result_binding(
        &self,
        submission: Submission,
        binding: ResultBinding,
    ) -> Result<SubmitResult, RuntimeError> {
        self.submit_inner(submission, Some(binding), None, |_| Ok(()))
    }

    fn submit_inner(
        &self,
        submission: Submission,
        result_binding: Option<ResultBinding>,
        publication: Option<Publication>,
        admit: impl FnOnce(&Id) -> Result<(), RuntimeError>,
    ) -> Result<SubmitResult, RuntimeError> {
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
        if !publication::valid(&descriptor, publication.as_ref()) {
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
        let next_order = order.checked_add(1).ok_or(RuntimeError::InvalidState)?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| admit(&id)))
            .map_err(|_| RuntimeError::InvalidState)??;
        table.next_order = next_order;
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
        let publication_gate = publication.as_ref().map(|_| Arc::new(Mutex::new(())));
        table.jobs.insert(
            id.clone(),
            Entry {
                descriptor,
                result_binding,
                submission: StoredSubmission {
                    kind_id,
                    deduplication_key,
                    priority,
                    scope_summary,
                    correlation_id,
                    sample,
                },
                body: Some(body),
                publication,
                publication_gate,
                state: JobState::Queued,
                sequence: 1,
                cancel: Arc::new(Cancellation(AtomicBool::new(false))),
                wake: Arc::new(Condvar::new()),
                live_flows: 0,
                paused_flows: 0,
                flows_aborted: false,
                started_at_ms: None,
                summary: None,
                order,
            },
        );
        Ok(SubmitResult::Created(id))
    }
    pub fn state(&self, id: &Id) -> Result<JobState, RuntimeError> {
        self.read_entry(id, |job| job.state)
    }

    /// Reads the state and terminal fields under the same table lock.
    pub fn snapshot(&self, id: &Id) -> Result<JobSnapshot, RuntimeError> {
        self.read_entry(id, |job| JobSnapshot {
            kind_id: job.descriptor.kind_id.clone(),
            result_binding: job.result_binding.clone(),
            state: job.state,
            sequence: job.sequence,
            result_version_id: job
                .summary
                .as_ref()
                .and_then(|summary| summary.result_version_id.clone()),
            failure_reason_key: job
                .summary
                .as_ref()
                .and_then(|summary| summary.failure_reason_key.clone()),
        })
    }

    fn read_entry<T>(&self, id: &Id, read: impl FnOnce(&Entry) -> T) -> Result<T, RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        expire(&mut table, now);
        table.jobs.get(id).map(read).ok_or(RuntimeError::NotFound)
    }
    pub fn cancel(&self, id: &Id) -> Result<CancelResult, RuntimeError> {
        self.cancel_with_sequence(id).map(|(result, _)| result)
    }

    /// Applies cancellation and returns the sequence of the acknowledged state.
    pub fn cancel_with_sequence(&self, id: &Id) -> Result<(CancelResult, u64), RuntimeError> {
        let gate = self.publication_gate(id)?;
        let _publication_guard = gate
            .as_ref()
            .map(|g| g.lock())
            .transpose()
            .map_err(|_| RuntimeError::Poisoned)?;
        self.cancel_under_gate(id)
    }

    fn cancel_under_gate(&self, id: &Id) -> Result<(CancelResult, u64), RuntimeError> {
        let now = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        expire(&mut table, now);
        let job = table.jobs.get_mut(id).ok_or(RuntimeError::NotFound)?;
        if job.state.is_terminal() {
            return Ok((CancelResult::AlreadyTerminal(job.state), job.sequence));
        }
        if !job.descriptor.controls.contains(&JobControl::Cancel) {
            return Ok((CancelResult::Unsupported(job.state), job.sequence));
        }
        if job.state == JobState::CancelRequested {
            return Ok((CancelResult::NoEffect(job.state), job.sequence));
        }
        job.cancel.0.store(true, Ordering::Release);
        job.wake.notify_all();
        if job.state == JobState::Queued {
            job.state = JobState::CancelRequested;
            let duplicate =
                if matches!(job.publication, Some(Publication::DuplicateTerminal { .. })) {
                    job.publication.take()
                } else {
                    None
                };
            drop(table);
            self.flow_limits.wake_waiters();
            if let Some(Publication::DuplicateTerminal { queued, commit }) = duplicate {
                let outcome =
                    publication::commit_duplicate(queued, commit, DuplicateTerminal::Cancelled);
                let sequence = self.finish(id, outcome)?;
                return Ok((CancelResult::Accepted(JobState::Cancelled), sequence));
            }
            let sequence = self.finish(
                id,
                JobOutcome {
                    counts: Default::default(),
                    completed_scope: false,
                    failure_reason_key: None,
                    unprocessed_summary: None,
                    result_version_id: None,
                },
            )?;
            return Ok((CancelResult::Accepted(JobState::Cancelled), sequence));
        }
        job.state = JobState::CancelRequested;
        job.sequence += 1;
        let sequence = job.sequence;
        let event = JobEvent::StateChanged {
            job_id: id.clone(),
            state: job.state,
            sequence,
        };
        let delivery = enqueue(&table, Delivery::Event(event));
        drop(table);
        self.flow_limits.wake_waiters();
        delivery?;
        Ok((CancelResult::Accepted(JobState::CancelRequested), sequence))
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
        let cancellation = job.cancel.clone();
        let body = job.body.take().ok_or(RuntimeError::InvalidState)?;
        let publication = job.publication.take();
        let gate = job.publication_gate.clone();
        drop(table);
        let context = Arc::new(flows::Context::new(
            id.clone(),
            self.table.clone(),
            cancellation,
            flows::Services {
                limits: self.flow_limits.clone(),
                clock: self.clock.clone(),
            },
        ));
        let body_context = context.clone();
        let mut outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body.run(body_context)))
                .unwrap_or_else(|_| JobOutcome {
                    counts: Default::default(),
                    completed_scope: false,
                    failure_reason_key: Some("support.internal_error".into()),
                    unprocessed_summary: None,
                    result_version_id: None,
                });
        drop(body);
        if !context.seal_and_wait(&mut outcome.counts) {
            outcome.completed_scope = false;
            outcome.failure_reason_key = Some("support.internal_error".into());
        }
        self.finish_with_publication(&id, outcome, gate, publication)?;
        Ok(Some(id))
    }
    fn finish(&self, id: &Id, outcome: JobOutcome) -> Result<u64, RuntimeError> {
        let finished_at_ms = self.clock.now_ms();
        let mut table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        let job = table.jobs.get_mut(id).ok_or(RuntimeError::NotFound)?;
        if job.state.is_terminal() {
            return Ok(job.sequence);
        }
        let state = final_state(job, &outcome);
        job.state = state;
        job.sequence += 2;
        let state_sequence = job.sequence - 1;
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
        let released_publication = job.publication.take();
        job.submission.deduplication_key = None;
        job.summary = Some(summary.clone());
        let record = job.descriptor.history != HistoryMode::None && !job.submission.sample;
        let delivery = enqueue(
            &table,
            Delivery::Terminal {
                summary: Box::new(summary),
                sequence: state_sequence,
                record,
            },
        );
        drop(table);
        drop(released_body);
        drop(released_publication);
        self.flow_limits.wake_waiters();
        delivery.map(|()| state_sequence + 1)
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

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RuntimeError {}

#[cfg(test)]
mod tests;
