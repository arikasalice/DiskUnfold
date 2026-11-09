use du_core_contracts::{
    Id,
    job::{Checkpoint, JobBody, JobContext, JobDescriptor, JobState, JobSummary},
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

pub trait JobIdentity: Send + Sync {
    fn next_id(&self) -> Id;
}
pub trait JobClock: Send + Sync {
    fn now_ms(&self) -> i64;
}
pub trait JobEvents: Send + Sync {
    fn publish(&self, event: JobEvent);
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobEvent {
    StateChanged {
        job_id: Id,
        state: JobState,
        sequence: u64,
    },
    Finished {
        summary: Box<JobSummary>,
        sequence: u64,
        history_recorded: bool,
        warning: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    KindUnknown,
    QueueFull,
    ShuttingDown,
    NotFound,
    DuplicateKind,
    InvalidDescriptor,
    InvalidState,
    Poisoned,
    DeliveryUnavailable,
    DeliveryBusy,
    ConsumerStart,
    ActiveJobs,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitResult {
    Created(Id),
    Existing(Id),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CancelResult {
    Accepted(JobState),
    NoEffect(JobState),
    Unsupported(JobState),
    AlreadyTerminal(JobState),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeduplicationKey {
    pub scope: String,
    pub input: String,
    pub channel: String,
    pub key: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    High,
    Normal,
    Low,
}
#[derive(Clone)]
pub struct Submission {
    pub kind_id: String,
    pub deduplication_key: Option<DeduplicationKey>,
    pub body: Arc<dyn JobBody>,
    pub priority: JobPriority,
    pub scope_summary: String,
    pub correlation_id: Id,
    pub sample: bool,
}
pub(crate) struct Cancellation(pub(crate) AtomicBool);
impl JobContext for Cancellation {
    fn checkpoint(&self) -> Checkpoint {
        if self.0.load(Ordering::Acquire) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
}
pub(crate) struct StoredSubmission {
    pub(crate) kind_id: String,
    pub(crate) deduplication_key: Option<DeduplicationKey>,
    pub(crate) priority: JobPriority,
    pub(crate) scope_summary: String,
    pub(crate) correlation_id: Id,
    pub(crate) sample: bool,
}
pub(crate) struct Entry {
    pub(crate) descriptor: JobDescriptor,
    pub(crate) submission: StoredSubmission,
    pub(crate) body: Option<Arc<dyn JobBody>>,
    pub(crate) state: JobState,
    pub(crate) sequence: u64,
    pub(crate) cancel: Arc<Cancellation>,
    pub(crate) started_at_ms: Option<i64>,
    pub(crate) summary: Option<JobSummary>,
    pub(crate) order: u64,
}
#[derive(Default)]
pub(crate) struct Table {
    pub(crate) kinds: HashMap<String, JobDescriptor>,
    pub(crate) jobs: HashMap<Id, Entry>,
    pub(crate) shutting_down: bool,
    pub(crate) next_order: u64,
    pub(crate) sender: Option<mpsc::Sender<Delivery>>,
}

pub struct JobRuntime {
    pub(crate) table: Arc<Mutex<Table>>,
    pub(crate) identity: Arc<dyn JobIdentity>,
    pub(crate) clock: Arc<dyn JobClock>,
    pub(crate) queue_limit: usize,
    pub(crate) consumer: Mutex<Option<JoinHandle<()>>>,
    pub(crate) outstanding: Arc<AtomicUsize>,
}

pub(crate) enum Delivery {
    Event(JobEvent),
    Terminal {
        summary: Box<JobSummary>,
        sequence: u64,
        record: bool,
    },
    #[cfg(test)]
    Fence(mpsc::Sender<()>),
    Stop(mpsc::Sender<()>),
}

pub(crate) fn expire(table: &mut Table, now: i64) {
    table.jobs.retain(|_, job| {
        job.summary
            .as_ref()
            .is_none_or(|summary| now.saturating_sub(summary.finished_at_ms) < 600_000)
    });
}
