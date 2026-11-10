use crate::delivery::enqueue;
use crate::model::{Cancellation, Delivery, JobClock, JobEvent, Limits, Table};
use du_core_contracts::{
    Id, Timestamp,
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError, JobState, ResourceClass},
};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct FlowState {
    sealed: bool,
    active: usize,
    abandoned: bool,
    counts: JobCounts,
}
struct Shared {
    state: Mutex<FlowState>,
    completed: Condvar,
}
pub(crate) struct Context {
    id: Id,
    table: Arc<Mutex<Table>>,
    cancellation: Arc<Cancellation>,
    services: Services,
    shared: Arc<Shared>,
}

/// Runtime services shared with a running job context.
pub(crate) struct Services {
    pub(crate) limits: Arc<Limits>,
    pub(crate) clock: Arc<dyn JobClock>,
}
impl Context {
    pub(crate) fn new(
        id: Id,
        table: Arc<Mutex<Table>>,
        cancellation: Arc<Cancellation>,
        services: Services,
    ) -> Self {
        Self {
            id,
            table,
            cancellation,
            services,
            shared: Arc::new(Shared {
                state: Mutex::new(FlowState::default()),
                completed: Condvar::new(),
            }),
        }
    }
    pub(crate) fn seal_and_wait(&self, counts: &mut JobCounts) -> bool {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.sealed = true;
        while state.active != 0 {
            state = self
                .shared
                .completed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        add_counts(counts, &state.counts);
        !state.abandoned
    }
}
impl JobContext for Context {
    fn utc_now(&self) -> Option<Timestamp> {
        Timestamp::from_unix_millis(self.services.clock.now_ms())
    }

    fn checkpoint(&self) -> Checkpoint {
        if self.cancellation.0.load(Ordering::Acquire) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        let table = self
            .table
            .lock()
            .map_err(|_| JobFlowError::DeliveryUnavailable)?;
        let job = table.jobs.get(&self.id).ok_or(JobFlowError::Closed)?;
        if self.cancellation.0.load(Ordering::Acquire) {
            return Err(JobFlowError::Cancelled);
        }
        if job.state != JobState::Running {
            return Err(JobFlowError::Closed);
        }
        let limits = &self.services.limits;
        if limits.config.metadata_io == 0
            || !job
                .descriptor
                .resource_classes
                .contains(&ResourceClass::MetadataIo)
        {
            return Err(JobFlowError::UnsupportedResource);
        }
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| JobFlowError::DeliveryUnavailable)?;
        if state.sealed {
            return Err(JobFlowError::Closed);
        }
        reserve(&limits.active, limits.config.metadata_io)?;
        state.active += 1;
        Ok(Box::new(Flow {
            id: self.id.clone(),
            table: self.table.clone(),
            limits: limits.clone(),
            shared: self.shared.clone(),
            counts: None,
        }))
    }
}
struct Flow {
    id: Id,
    table: Arc<Mutex<Table>>,
    limits: Arc<Limits>,
    shared: Arc<Shared>,
    counts: Option<JobCounts>,
}
impl JobFlow for Flow {
    fn report_item(&self, payload: &[u8]) -> Result<(), JobFlowError> {
        if payload.len() > self.limits.config.item_bytes {
            return Err(JobFlowError::PayloadTooLarge);
        }
        reserve(&self.limits.pending, self.limits.config.queued_items)?;
        let result = self.send_item(payload);
        if result.is_err() {
            self.limits.pending.fetch_sub(1, Ordering::AcqRel);
        }
        result
    }
    fn finish(mut self: Box<Self>, counts: JobCounts) {
        self.counts = Some(counts);
    }
}
impl Flow {
    fn send_item(&self, payload: &[u8]) -> Result<(), JobFlowError> {
        let mut table = self
            .table
            .lock()
            .map_err(|_| JobFlowError::DeliveryUnavailable)?;
        let job = table.jobs.get(&self.id).ok_or(JobFlowError::Closed)?;
        if job.state.is_terminal() {
            return Err(JobFlowError::Closed);
        }
        let sequence = job.sequence + 1;
        let event = JobEvent::ItemResult {
            job_id: self.id.clone(),
            kind_id: job.descriptor.kind_id.clone(),
            sequence,
            payload: payload.to_vec(),
        };
        enqueue(&table, Delivery::Item(event, self.limits.pending.clone()))
            .map_err(|_| JobFlowError::DeliveryUnavailable)?;
        let job = table.jobs.get_mut(&self.id).ok_or(JobFlowError::Closed)?;
        job.sequence = sequence;
        Ok(())
    }
}
impl Drop for Flow {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(counts) = &self.counts {
            add_counts(&mut state.counts, counts);
        } else {
            state.abandoned = true;
        }
        state.active -= 1;
        self.limits.active.fetch_sub(1, Ordering::AcqRel);
        self.shared.completed.notify_all();
    }
}
fn reserve(counter: &AtomicUsize, limit: usize) -> Result<(), JobFlowError> {
    counter
        .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < limit).then(|| n + 1)
        })
        .map(|_| ())
        .map_err(|_| JobFlowError::Busy)
}
fn add_counts(a: &mut JobCounts, b: &JobCounts) {
    a.completed = a.completed.saturating_add(b.completed);
    a.overwritten = a.overwritten.saturating_add(b.overwritten);
    a.failed = a.failed.saturating_add(b.failed);
    a.skipped = a.skipped.saturating_add(b.skipped);
    a.unprocessed = a.unprocessed.saturating_add(b.unprocessed);
}
