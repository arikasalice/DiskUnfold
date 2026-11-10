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
pub trait JobHistory: Send + Sync {
    fn append(&self, summary: &JobSummary) -> Result<HistoryAppend, String>;
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
    fn report_item(&self, payload: &[u8]) -> Result<(), JobFlowError>;
    fn finish(self: Box<Self>, counts: JobCounts);
}
pub trait JobContext: Send + Sync {
    fn checkpoint(&self) -> Checkpoint;
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
