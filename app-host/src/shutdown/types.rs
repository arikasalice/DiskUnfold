//! Shared shutdown types: lifecycle values, gateway reports, ports and errors.

use std::time::{Duration, Instant, UNIX_EPOCH};

use du_core_contracts::Id;
use du_engine_api::shutdown::{HistoryCommitStatus, ShutdownAdmission, ShutdownPreparation};

/// Current UTC timestamp in Unix milliseconds for the shutdown state sequence.
pub(crate) fn current_time_ms() -> i64 {
    UNIX_EPOCH
        .elapsed()
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

/// How the user asked to leave. Both modes close admission and cancel every job
/// (F24); `WithoutWaiting` only shortens how long the host waits for bodies to
/// return from their current OS call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuitMode {
    Waiting,
    WithoutWaiting,
}

/// The only result returned by the atomic app.quit admission cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuitAdmission {
    Accepted { active_change_jobs: usize },
    ConfirmRequired { active_change_jobs: usize },
    AlreadyClosing,
}

impl From<ShutdownAdmission> for QuitAdmission {
    fn from(value: ShutdownAdmission) -> Self {
        match value {
            ShutdownAdmission::Accepted { active_change_jobs } => {
                Self::Accepted { active_change_jobs }
            }
            ShutdownAdmission::ConfirmRequired { active_change_jobs } => {
                Self::ConfirmRequired { active_change_jobs }
            }
            ShutdownAdmission::AlreadyClosing => Self::AlreadyClosing,
        }
    }
}

/// Listener closure states distinguish an absent transport from a closed one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListenerClose {
    /// No listener has been installed in this composition.
    NotInstalled,
    /// The listener and every accepted socket have been closed.
    #[allow(dead_code)] // Reserved for the F12 concrete listener adapter.
    Closed,
    /// The close failed and the host moved on after force-closing the gate.
    Unconfirmed,
}

/// IF-12 outcome: the gate state, listener state and the counts it retired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GatewayCloseReport {
    /// True only after new gateway sessions and requests are rejected.
    pub access_closed: bool,
    /// Listener state; `NotInstalled` does not claim a socket was closed.
    pub listener: ListenerClose,
    /// True when the graceful close deadline elapsed before the close returned.
    pub timed_out: bool,
    /// Sessions that were open when access closed (IF-12 "closed sessions").
    pub sessions_closed: usize,
    /// Queued responses discarded before delivery (IF-12 "discarded responses").
    pub responses_discarded: usize,
    /// Optional diagnostic that does not prevent shutdown after force-close.
    pub detail: Option<String>,
}

/// App lifecycle status sent with monotonic sequence and observation time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LifecycleStatus {
    Starting,
    Running,
    AccessClosed,
    Draining,
    Closed,
}

/// One host-owned lifecycle event; the IPC adapter maps it to the shared IF-07 type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LifecycleEvent {
    pub status: LifecycleStatus,
    pub previous: LifecycleStatus,
    pub remaining_tasks: usize,
    pub sequence: u64,
    pub observed_at_ms: i64,
}

/// How the shutdown ended, used to choose the process exit status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShutdownOutcome {
    /// Every step ran in order and no job was left behind.
    Clean,
    /// The user chose to leave without waiting and these jobs were abandoned
    /// without a success record. Their storage is left for next-start recovery.
    Abandoned { jobs: usize },
    /// A waiting shutdown hit its drain bound with jobs still running.
    DrainTimedOut { jobs: usize },
    /// A step kept failing until its retry bound ended the shutdown.
    Forced(String),
}

/// Narrow app-host ports used by the shutdown state machine.
///
/// `close_mcp_access` closes request admission and asks the transport adapter
/// to close its listener. `NotInstalled` is distinct from a confirmed socket
/// close; a timeout is acceptable only after the adapter confirms closure.
/// `release_engine` drops the app-host engine owner after the history barrier
/// and worker join; the database port may close only after all other owners
/// have also dropped.
pub(crate) trait ShutdownPorts: Send {
    fn close_job_admission(&mut self, confirmed: bool) -> Result<QuitAdmission, String>;
    fn close_mcp_access(&mut self, deadline: Instant) -> Result<GatewayCloseReport, String>;
    /// Closes the access gate without waiting on the listener. Returns whether
    /// the gate is closed. The default reports that nothing could be forced.
    fn force_close_mcp_access(&mut self) -> bool {
        false
    }
    fn prepare_engine_shutdown(&mut self) -> Result<ShutdownPreparation, String>;
    fn list_active_jobs(&mut self) -> Result<Vec<Id>, String>;
    fn begin_history_commit_barrier(&mut self, timeout: Duration) -> Result<(), String>;
    fn poll_history_commit_barrier(&mut self) -> HistoryCommitStatus;
    fn history_failure_count(&self) -> Option<usize>;
    fn close_job_workers(&mut self) -> Result<(), String>;
    fn release_engine(&mut self);
    fn checkpoint_and_close_database(&mut self) -> Result<(), String>;
    fn zeroize_secrets(&mut self) -> Result<(), String>;
    fn publish_lifecycle(&mut self, event: LifecycleEvent);
    /// Whether every published lifecycle event has left the host's queue.
    fn final_events_flushed(&self) -> bool {
        true
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ShutdownError {
    Admission(String),
    Gateway(String),
    GatewayAccessOpen(Option<String>),
    Engine(String),
    WatcherStopUnconfirmed,
    JobList(String),
    HistoryBarrier(String),
    HistoryBarrierMissing,
    HistoryCommitTimedOut,
    HistoryCommitFailed {
        failed_appends: Option<usize>,
    },
    WorkerStop(String),
    DatabaseClose(String),
    SecretClear(String),
    SequenceExhausted,
    /// The coordinator lock was poisoned by a panic; shutdown cannot be trusted.
    Poisoned,
    /// A waiting shutdown ended because jobs outlived the drain bound.
    DrainTimedOut {
        remaining: usize,
    },
    /// A phase failed on every bounded retry and the host forced the exit.
    RetriesExhausted {
        phase: &'static str,
        last: String,
    },
}

impl std::fmt::Display for ShutdownError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ShutdownError {}
