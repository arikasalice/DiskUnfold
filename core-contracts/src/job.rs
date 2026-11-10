//! In-process job contracts shared by runtimes and job bodies.
use crate::{Id, Timestamp};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    PauseRequested,
    Paused,
    CancelRequested,
    Done,
    Partial,
    Failed,
    Cancelled,
}
impl JobState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Partial | Self::Failed | Self::Cancelled
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobControl {
    Pause,
    Resume,
    Cancel,
    SetPriority,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceClass {
    MetadataIo,
    ContentIo,
    FileChange,
    Output,
    Compute,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryMode {
    PerItem,
    Summary,
    None,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressUnit {
    Items,
    Bytes,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobDescriptor {
    pub kind_id: String,
    pub version: u32,
    pub display_name_key: String,
    pub controls: Vec<JobControl>,
    pub safe_points: Vec<String>,
    pub progress_unit: ProgressUnit,
    pub resource_classes: Vec<ResourceClass>,
    pub history: HistoryMode,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobCounts {
    pub completed: u64,
    pub overwritten: u64,
    pub failed: u64,
    pub skipped: u64,
    pub unprocessed: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobSummary {
    pub job_id: Id,
    pub kind_id: String,
    pub kind_version: u32,
    pub scope_summary: String,
    pub state: JobState,
    pub counts: JobCounts,
    pub failure_reason_key: Option<String>,
    pub unprocessed_summary: Option<String>,
    pub result_version_id: Option<Id>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: i64,
    pub correlation_id: Id,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryAppend {
    Added,
    Duplicate,
}

/// The history revision committed by an append transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryRevision {
    /// The deletion generation visible to the committing transaction.
    pub delete_generation: u64,
    /// The maximum committed record sequence at that revision.
    pub max_record_seq: u64,
}

/// Evidence captured by the same transaction that inserted a terminal record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryAppendReceipt {
    /// The revision committed with the new record.
    pub revision: HistoryRevision,
    /// The UTC observation time associated with the committed append.
    pub observed_at_ms: i64,
}

/// An append result, with a receipt only when one can be proven by the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryAppendOutcome {
    /// A new record was inserted; legacy stores may omit its receipt.
    Added(Option<HistoryAppendReceipt>),
    /// The record already existed and no new change was committed.
    Duplicate,
}

impl HistoryAppendOutcome {
    /// Returns the append status for callers that do not need a receipt.
    pub fn status(self) -> HistoryAppend {
        match self {
            Self::Added(_) => HistoryAppend::Added,
            Self::Duplicate => HistoryAppend::Duplicate,
        }
    }
}

/// A committed change that invalidates a previously read history revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryChanged {
    /// The newly committed history revision.
    pub revision: HistoryRevision,
    /// The kind of history change.
    pub reason: HistoryChangeReason,
    /// The number of records affected by the change.
    pub count: u64,
    /// The UTC observation time, in milliseconds since the Unix epoch.
    pub observed_at_ms: i64,
    /// References removed by the change, for in-process consumers only.
    pub deleted_job_ids: Vec<Id>,
}

/// History change reasons supported by this in-process notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryChangeReason {
    /// A terminal history record was appended.
    Appended,
    /// A startup check marked unfinished operations as interrupted.
    Interrupted,
}

/// The persisted outcome of a startup interruption check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryInterruptionReceipt {
    /// Revision committed with the interruption markers.
    pub revision: HistoryRevision,
    /// Number of newly marked operations.
    pub count: u64,
    /// UTC time used to mark the interrupted operations.
    pub observed_at_ms: i64,
}

/// A startup-only storage port for unfinished operations from a previous session.
pub trait HistoryRecovery {
    /// Returns a receipt only when new interruption markers were committed.
    fn mark_interrupted(&self, now_ms: i64) -> Result<Option<HistoryInterruptionReceipt>, String>;
}

pub trait JobHistory: Send + Sync {
    fn append(&self, summary: &JobSummary) -> Result<HistoryAppend, String>;

    /// Append and return a transaction receipt when supported by the adapter.
    /// Legacy adapters report their result without inventing a revision.
    fn append_with_receipt(&self, summary: &JobSummary) -> Result<HistoryAppendOutcome, String> {
        self.append(summary).map(|result| match result {
            HistoryAppend::Added => HistoryAppendOutcome::Added(None),
            HistoryAppend::Duplicate => HistoryAppendOutcome::Duplicate,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checkpoint {
    Continue,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobFlowError {
    Busy,
    Cancelled,
    Closed,
    UnsupportedResource,
    PayloadTooLarge,
    DeliveryUnavailable,
}
/// One registered metadata I/O flow. Dropping it releases its permit.
pub trait JobFlow: Send {
    /// Waits at a completed unit boundary; returns `Cancel` when the flow must stop.
    /// Call without holding queue locks, bounded sender permits, or other resources needed by other flows to reach their boundary.
    fn safe_point(&self) -> Checkpoint {
        Checkpoint::Continue
    }
    fn report_item(&self, payload: &[u8]) -> Result<(), JobFlowError>;
    fn finish(self: Box<Self>, counts: JobCounts);
}
pub trait JobContext: Send + Sync {
    fn checkpoint(&self) -> Checkpoint;
    /// Wakes parked metadata flows after a body failure and closes admission to new flows.
    /// The body still reports its failure outcome. This does not request user cancellation
    /// or interrupt metadata calls already in progress.
    fn abort_flows(&self) {}
    /// Supplies UTC time to bodies recording evidence. A context without a clock may omit it.
    fn utc_now(&self) -> Option<Timestamp> {
        None
    }
    /// Monotonic elapsed time from a clock-specific origin, when available.
    fn monotonic_now(&self) -> Option<std::time::Duration> {
        None
    }
    /// Returns Busy rather than spawning or waiting when no metadata I/O permit is available.
    /// Returns UnsupportedResource when metadata I/O is disabled or not declared for the job.
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError>;
}
/// The body can return while registered flows finish; the runtime waits for them.
pub trait JobBody: Send + Sync {
    fn kind_id(&self) -> &str;
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobOutcome {
    pub counts: JobCounts,
    pub completed_scope: bool,
    pub failure_reason_key: Option<String>,
    pub unprocessed_summary: Option<String>,
    pub result_version_id: Option<Id>,
}
