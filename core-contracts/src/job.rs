//! In-process job contracts shared by runtimes and job bodies.
use crate::{Id, Timestamp, scan_observation::DeviceKind};
use std::{fmt, sync::Arc};

mod lane_status;
pub use lane_status::{LaneReport, LaneStatusReport, LaneWait};
mod history_retention;
pub use history_retention::{
    HistoryExpiryCandidate, HistoryExpiryCandidates, HistoryExpiryRead, HistoryExpiryReadError,
};
mod session_undo_info;
pub use session_undo_info::{SessionUndoInfo, UndoInfoUnavailable};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
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
/// The unit in which a job kind counts its progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ProgressUnit {
    Items,
    Bytes,
}

/// One progress report from a job body to its runtime (F24 IF-13).
///
/// Amounts use the unit declared by the job kind's descriptor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JobProgress {
    /// Units processed so far.
    pub completed: u64,
    /// The confirmed total, or none while the total is not established.
    pub total: Option<u64>,
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
/// Outcome counts of one job.
///
/// Every processed unit is counted in exactly one field. The amount of work
/// done, as progress reports it, is `completed + failed`; that sum never
/// decreases during a job. `completed` alone can decrease by one when a unit
/// that was counted as completed turns out to have failed, for example a
/// listed folder whose contents could not be read afterwards.
///
/// For `scan.metadata` a unit is one recorded item of the scope, and
/// `failed` equals the number of paths with a scope gap in the result. A
/// scope root is an item and is counted once when the collector admits it:
/// as failed when its identity could not be verified, otherwise as completed.
/// A root that later could not be read moves from completed to failed. So
/// `completed + failed` equals the number of items of the result, unless the
/// job was cancelled before its roots were admitted; then nothing is counted.
/// When `scan.metadata` ends without publishing a result, `completed` is 0 and
/// the items it counted are reported as `unprocessed`, so its terminal
/// `completed + failed` can be lower than its last progress report. The sample
/// load (`scan.sample`) does the same when it releases its versions.
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
    /// Root display strings captured when the job was submitted, when applicable.
    pub scope_roots: Vec<String>,
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

/// Opaque key used to acquire one process-wide in-flight seat for a disk.
/// The key is intentionally excluded from formatting and wire contracts.
#[derive(Clone)]
pub struct DiskSeat {
    kind: DeviceKind,
    key: Vec<u8>,
}

impl DiskSeat {
    /// Creates a disk seat request; empty keys are not accepted.
    pub fn new(kind: DeviceKind, key: Vec<u8>) -> Self {
        Self { kind, key }
    }

    /// Device class used to select the process-wide permit.
    pub fn kind(&self) -> DeviceKind {
        self.kind
    }

    /// Opaque bytes used only as an in-process map key.
    pub fn key(&self) -> &[u8] {
        &self.key
    }
}

impl fmt::Debug for DiskSeat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiskSeat")
            .field("kind", &self.kind)
            .field("key", &"<redacted>")
            .finish()
    }
}

/// RAII token for one device seat; dropping it releases the runtime permit.
pub struct DeviceSeat(Option<Box<dyn FnOnce() + Send + 'static>>);

impl DeviceSeat {
    /// Creates a runtime-owned release token.
    pub fn new(release: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(release)))
    }

    /// Creates an unrestricted token for contexts that do not implement gating.
    pub fn unrestricted() -> Self {
        Self(None)
    }
}

impl Drop for DeviceSeat {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            release();
        }
    }
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
    /// Reports the body's latest progress without blocking (F24 IF-13).
    ///
    /// A later report replaces an earlier one. The runtime publishes at most one
    /// value per job and batch interval, so intermediate values may be skipped,
    /// and it discards reports that arrive after the job's terminal state. The
    /// default ignores the report for contexts that do not deliver progress.
    fn report_progress(&self, _progress: JobProgress) {}
    /// Reports the body's latest per-lane state without blocking (F02 IF-17).
    ///
    /// The same rules as [`JobContext::report_progress`] apply: a later report
    /// replaces an earlier one, at most one value per job and batch interval is
    /// published, and reports after the job's terminal state are discarded. The
    /// default ignores the report.
    fn report_lane_status(&self, _status: LaneStatusReport) {}
    /// Returns Busy rather than spawning or waiting when no metadata I/O permit is available.
    /// Returns UnsupportedResource when metadata I/O is disabled or not declared for the job.
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError>;
    /// Waits cooperatively for a flow permit, without holding a permit while waiting.
    ///
    /// Call without a file handle, queue lock, or bounded-channel permit. A
    /// runtime implementation wakes on permit release, resume, cancellation,
    /// abort, or closure. The default preserves nonblocking implementations and
    /// may return `Busy`; callers must not spin when that happens.
    fn wait_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        self.try_start_flow()
    }
    /// Tries to reserve a process-wide disk seat; `None` means that disk is full.
    /// Existing contexts remain unrestricted by default.
    fn try_device_seat(&self, _disk: &DiskSeat) -> Result<Option<DeviceSeat>, JobFlowError> {
        Ok(Some(DeviceSeat::unrestricted()))
    }
    /// Cooperatively waits without holding queue locks for a disk seat.
    /// The default does not wait and returns `Busy` if an implementation gates seats.
    fn wait_device_seat(&self, disk: &DiskSeat) -> Result<DeviceSeat, JobFlowError> {
        self.try_device_seat(disk)?.ok_or(JobFlowError::Busy)
    }
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
