use du_core_contracts::{
    Id,
    job::{HistoryChanged, JobBody, JobDescriptor, JobState, JobSummary},
    result_events::ResultVersionNotice,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize},
        mpsc,
    },
    thread::JoinHandle,
};

pub trait JobIdentity: Send + Sync {
    fn next_id(&self) -> Id;
}
pub trait JobClock: Send + Sync {
    fn now_ms(&self) -> i64;
    /// Elapsed time from a stable, monotonic origin, if supported.
    fn monotonic_now(&self) -> Option<std::time::Duration> {
        None
    }
}
pub trait JobEvents: Send + Sync {
    fn publish(&self, event: JobEvent);

    /// Deliver a committed history invalidation to interested subscribers.
    fn publish_history_changed(&self, _event: HistoryChanged) {}

    /// Reports expiry after all earlier delivery for this job has finished.
    fn retire_job(&self, _id: &Id) {}
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobEvent {
    ResultVersionChanged(Box<ResultVersionNotice>),
    StateChanged {
        job_id: Id,
        state: JobState,
        sequence: u64,
    },
    ItemResult {
        job_id: Id,
        kind_id: String,
        sequence: u64,
        payload: Vec<u8>,
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
/// Identifies the lineage and scope associated with a result-producing job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultBinding {
    pub lineage_id: Id,
    pub scope_version: Id,
}

/// A consistent read of the job state and its published terminal result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobSnapshot {
    pub kind_id: String,
    pub state: JobState,
    pub sequence: u64,
    pub result_version_id: Option<Id>,
    pub failure_reason_key: Option<String>,
    pub result_binding: Option<ResultBinding>,
    /// The confirmed terminal result, absent until history handling completes.
    pub terminal: Option<TerminalSnapshot>,
}

/// The immutable result shared by terminal events and subsequent reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub summary: JobSummary,
    pub sequence: u64,
    pub history_recorded: bool,
    pub warning: Option<String>,
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
pub(crate) type RetiredVizJob = (Option<Arc<dyn JobBody>>, Option<Publication>);
pub(crate) enum Publication {
    Query(Box<dyn FnOnce() -> Result<Id, String> + Send>),
    Visualization {
        commit: Box<dyn FnOnce() -> Result<(), String> + Send>,
        retained: Option<Arc<dyn Send + Sync>>,
    },
    DuplicateTerminal {
        queued: du_core_contracts::job::JobOutcome,
        commit: Box<dyn FnOnce(crate::publication::DuplicateTerminal) -> Result<Id, String> + Send>,
    },
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
    pub(crate) result_binding: Option<ResultBinding>,
    pub(crate) submission: StoredSubmission,
    pub(crate) body: Option<Arc<dyn JobBody>>,
    pub(crate) publication: Option<Publication>,
    pub(crate) publication_gate: Option<Arc<Mutex<()>>>,
    pub(crate) state: JobState,
    pub(crate) sequence: u64,
    pub(crate) cancel: Arc<Cancellation>,
    pub(crate) wake: Arc<Condvar>,
    pub(crate) live_flows: usize,
    pub(crate) paused_flows: usize,
    pub(crate) flows_aborted: bool,
    pub(crate) started_at_ms: Option<i64>,
    pub(crate) summary: Option<JobSummary>,
    pub(crate) terminal: Option<TerminalOutcome>,
    pub(crate) order: u64,
}
pub(crate) struct TerminalOutcome {
    pub(crate) sequence: u64,
    pub(crate) history_recorded: bool,
    pub(crate) warning: Option<String>,
}
impl Entry {
    pub(crate) fn snapshot(&self) -> JobSnapshot {
        JobSnapshot {
            kind_id: self.descriptor.kind_id.clone(),
            result_binding: self.result_binding.clone(),
            state: self.state,
            terminal: self.terminal_snapshot(),
            sequence: self.sequence,
            result_version_id: self
                .summary
                .as_ref()
                .and_then(|s| s.result_version_id.clone()),
            failure_reason_key: self
                .summary
                .as_ref()
                .and_then(|s| s.failure_reason_key.clone()),
        }
    }
    pub(crate) fn terminal_snapshot(&self) -> Option<TerminalSnapshot> {
        self.terminal
            .as_ref()
            .zip(self.summary.as_ref())
            .map(|(outcome, summary)| TerminalSnapshot {
                summary: summary.clone(),
                sequence: outcome.sequence,
                history_recorded: outcome.history_recorded,
                warning: outcome.warning.clone(),
            })
    }
}
#[derive(Default)]
pub(crate) struct Table {
    pub(crate) kinds: HashMap<String, JobDescriptor>,
    pub(crate) jobs: HashMap<Id, Entry>,
    pub(crate) shutting_down: bool,
    pub(crate) shutdown_delivery_started: bool,
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
    pub(crate) flow_limits: Arc<Limits>,
}

pub(crate) enum Delivery {
    Event(JobEvent),
    Item(JobEvent, Arc<AtomicUsize>),
    Terminal {
        summary: Box<JobSummary>,
        sequence: u64,
        record: bool,
    },
    Retired(Id),
    #[cfg(test)]
    Fence(mpsc::Sender<()>),
    Stop(mpsc::Sender<usize>),
}

pub(crate) fn expire(table: &mut Table, now: i64) {
    let sender = table.sender.as_ref();
    table.jobs.retain(|id, job| {
        let retain = job.summary.as_ref().is_none_or(|summary| {
            job.terminal.is_none() || now.saturating_sub(summary.finished_at_ms) < 600_000
        });
        if !retain && let Some(sender) = sender {
            // Expiry shares the job-table lock and delivery order with terminal events.
            let _ = sender.send(Delivery::Retired(id.clone()));
        }
        retain
    });
}

/// Limits are supplied by the composition point; zero disables that facility.
#[derive(Clone, Copy, Debug)]
pub struct FlowLimits {
    pub metadata_io: usize,
    pub queued_items: usize,
    pub item_bytes: usize,
}
impl FlowLimits {
    pub const fn disabled() -> Self {
        Self {
            metadata_io: 0,
            queued_items: 0,
            item_bytes: 0,
        }
    }
}
pub(crate) struct Limits {
    pub(crate) config: FlowLimits,
    pub(crate) active: AtomicUsize,
    pub(crate) content_limit: usize,
    pub(crate) content_active: AtomicUsize,
    pub(crate) wait_lock: Mutex<()>,
    pub(crate) wait_changed: Condvar,
    #[cfg(test)]
    pub(crate) waiters: AtomicUsize,
    pub(crate) pending: Arc<AtomicUsize>,
}
impl Limits {
    pub(crate) fn new(config: FlowLimits, content_limit: usize) -> Self {
        Self {
            config,
            active: AtomicUsize::new(0),
            content_limit,
            content_active: AtomicUsize::new(0),
            wait_lock: Mutex::new(()),
            wait_changed: Condvar::new(),
            #[cfg(test)]
            waiters: AtomicUsize::new(0),
            pending: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) fn wake_waiters(&self) {
        let _guard = self
            .wait_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.wait_changed.notify_all();
    }
}
