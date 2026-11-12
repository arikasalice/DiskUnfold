//! Ordered app-host shutdown orchestration.
//!
//! This module is intentionally not registered by `lib.rs` or `lifecycle.rs`.
//! The owner of those files must connect the coordinator to the actual window,
//! gateway transport, engine and final database owner.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use du_core_contracts::Id;
use du_engine_api::shutdown::{HistoryCommitStatus, ShutdownAdmission, ShutdownPreparation};

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

/// A gateway outcome that always states whether the listener and sockets are closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GatewayCloseReport {
    /// True only after the acceptor and all accepted sockets are closed.
    pub socket_closed: bool,
    /// True when the graceful close deadline elapsed and force-close was used.
    pub timed_out: bool,
    /// Optional diagnostic that does not prevent shutdown after force-close.
    pub detail: Option<String>,
}

/// App lifecycle status sent with monotonic sequence and observation time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LifecycleStatus {
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

/// Narrow app-host ports used by the shutdown state machine.
///
/// `close_mcp_access` must honor its deadline and force-close the acceptor and
/// every socket before returning a report with `socket_closed = true`. It must
/// not report a graceful timeout as closure while any socket remains open.
/// `release_engine` drops the app-host engine owner after the history barrier
/// and worker join; the database port may close only after all other owners
/// have also dropped.
pub(crate) trait ShutdownPorts {
    fn close_job_admission(&mut self, confirmed: bool) -> Result<QuitAdmission, String>;
    fn close_mcp_access(&mut self, deadline: Instant) -> Result<GatewayCloseReport, String>;
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Running,
    ClosingAccess,
    AccessClosed,
    Draining,
    HistoryCommit,
    ClosingStorage,
    ZeroizingSecrets,
    Closed,
}

/// Ordered coordinator for the waiting shutdown path.
///
/// The non-waiting exit path is deliberately not exposed here. F15 requires
/// unfinished work to remain unrecorded, while F24 terminal cancellation runs
/// the ordinary history path. Those contracts need reconciliation before that
/// mode can be connected safely.
pub(crate) struct ShutdownCoordinator<P> {
    ports: P,
    phase: Phase,
    active_jobs: HashSet<Id>,
    preparation: Option<ShutdownPreparation>,
    history_barrier_started: bool,
    engine_released: bool,
    sequence: u64,
    visible_status: LifecycleStatus,
}

impl<P: ShutdownPorts> ShutdownCoordinator<P> {
    pub(crate) fn new(ports: P) -> Self {
        Self {
            ports,
            phase: Phase::Running,
            active_jobs: HashSet::new(),
            preparation: None,
            history_barrier_started: false,
            engine_released: false,
            sequence: 0,
            visible_status: LifecycleStatus::Running,
        }
    }

    /// Atomically admits a waiting quit request, or returns confirmation without mutation.
    pub(crate) fn request_waiting_quit(
        &mut self,
        confirmed: bool,
    ) -> Result<QuitAdmission, ShutdownError> {
        if self.phase != Phase::Running {
            return Ok(QuitAdmission::AlreadyClosing);
        }
        let admission = self
            .ports
            .close_job_admission(confirmed)
            .map_err(ShutdownError::Admission)?;
        match admission {
            QuitAdmission::Accepted { .. } => self.phase = Phase::ClosingAccess,
            QuitAdmission::ConfirmRequired { .. } | QuitAdmission::AlreadyClosing => {}
        }
        Ok(admission)
    }

    /// Advances one bounded step. Gateway close receives a two-second deadline;
    /// timeout is acceptable only when the adapter confirms forced socket closure.
    pub(crate) fn advance(&mut self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        match self.phase {
            Phase::Running | Phase::Closed => Ok(()),
            Phase::ClosingAccess => self.close_access(observed_at_ms),
            Phase::AccessClosed => self.prepare_and_drain(observed_at_ms),
            Phase::Draining => self.begin_history_barrier(observed_at_ms),
            Phase::HistoryCommit => self.finish_history_and_close_workers(observed_at_ms),
            Phase::ClosingStorage => self.close_database(observed_at_ms),
            Phase::ZeroizingSecrets => self.clear_secrets(observed_at_ms),
        }
    }

    /// Applies one F24 terminal event. Duplicate or unknown references do not decrement twice.
    pub(crate) fn job_finished(
        &mut self,
        job_ref: &Id,
        observed_at_ms: i64,
    ) -> Result<(), ShutdownError> {
        if self.phase != Phase::Draining || !self.active_jobs.remove(job_ref) {
            return Ok(());
        }
        self.emit(LifecycleStatus::Draining, observed_at_ms)
    }

    /// Rebuilds the active set from authoritative `job.list` after a resync signal.
    pub(crate) fn resync_active_jobs(&mut self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        if self.phase != Phase::Draining {
            return Ok(());
        }
        let next: HashSet<_> = self
            .ports
            .list_active_jobs()
            .map_err(ShutdownError::JobList)?
            .into_iter()
            .collect();
        if self.active_jobs == next {
            return Ok(());
        }
        self.active_jobs = next;
        self.emit(LifecycleStatus::Draining, observed_at_ms)
    }

    pub(crate) fn remaining_tasks(&self) -> usize {
        self.active_jobs.len()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.phase == Phase::Closed
    }

    pub(crate) fn into_ports(self) -> P {
        self.ports
    }

    fn close_access(&mut self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        let report = self
            .ports
            .close_mcp_access(Instant::now() + Duration::from_secs(2))
            .map_err(ShutdownError::Gateway)?;
        if !report.socket_closed {
            return Err(ShutdownError::GatewayStillOpen(report.detail));
        }
        self.phase = Phase::AccessClosed;
        self.emit(LifecycleStatus::AccessClosed, observed_at_ms)
    }

    fn prepare_and_drain(&mut self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        let preparation = self
            .ports
            .prepare_engine_shutdown()
            .map_err(ShutdownError::Engine)?;
        let watch_stop = preparation.watch_stop;
        self.preparation = Some(preparation);
        if watch_stop == du_engine_api::shutdown::WatchStopResult::Failed {
            return Err(ShutdownError::WatcherStopUnconfirmed);
        }
        self.active_jobs = self
            .ports
            .list_active_jobs()
            .map_err(ShutdownError::JobList)?
            .into_iter()
            .collect();
        self.phase = Phase::Draining;
        self.emit(LifecycleStatus::Draining, observed_at_ms)
    }

    fn begin_history_barrier(&mut self, _observed_at_ms: i64) -> Result<(), ShutdownError> {
        if !self.active_jobs.is_empty() {
            return Ok(());
        }
        self.ports
            .begin_history_commit_barrier(Duration::from_secs(30))
            .map_err(ShutdownError::HistoryBarrier)?;
        self.history_barrier_started = true;
        self.phase = Phase::HistoryCommit;
        Ok(())
    }

    fn finish_history_and_close_workers(
        &mut self,
        _observed_at_ms: i64,
    ) -> Result<(), ShutdownError> {
        if !self.history_barrier_started {
            return Err(ShutdownError::HistoryBarrierMissing);
        }
        match self.ports.poll_history_commit_barrier() {
            HistoryCommitStatus::Pending => Ok(()),
            HistoryCommitStatus::TimedOut => Err(ShutdownError::HistoryCommitTimedOut),
            HistoryCommitStatus::Failed => Err(ShutdownError::HistoryCommitFailed {
                failed_appends: self.ports.history_failure_count(),
            }),
            HistoryCommitStatus::Complete => {
                self.ports
                    .close_job_workers()
                    .map_err(ShutdownError::WorkerStop)?;
                if !self.engine_released {
                    self.ports.release_engine();
                    self.engine_released = true;
                }
                self.phase = Phase::ClosingStorage;
                Ok(())
            }
        }
    }

    fn close_database(&mut self, _observed_at_ms: i64) -> Result<(), ShutdownError> {
        self.ports
            .checkpoint_and_close_database()
            .map_err(ShutdownError::DatabaseClose)?;
        self.phase = Phase::ZeroizingSecrets;
        Ok(())
    }

    fn clear_secrets(&mut self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        self.ports
            .zeroize_secrets()
            .map_err(ShutdownError::SecretClear)?;
        self.phase = Phase::Closed;
        self.emit(LifecycleStatus::Closed, observed_at_ms)
    }

    fn emit(&mut self, status: LifecycleStatus, observed_at_ms: i64) -> Result<(), ShutdownError> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(ShutdownError::SequenceExhausted)?;
        self.sequence = sequence;
        let previous = self.visible_status;
        self.visible_status = status;
        self.ports.publish_lifecycle(LifecycleEvent {
            status,
            previous,
            remaining_tasks: self.active_jobs.len(),
            sequence,
            observed_at_ms,
        });
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ShutdownError {
    Admission(String),
    Gateway(String),
    GatewayStillOpen(Option<String>),
    Engine(String),
    WatcherStopUnconfirmed,
    JobList(String),
    HistoryBarrier(String),
    HistoryBarrierMissing,
    HistoryCommitTimedOut,
    HistoryCommitFailed { failed_appends: Option<usize> },
    WorkerStop(String),
    DatabaseClose(String),
    SecretClear(String),
    SequenceExhausted,
}

impl std::fmt::Display for ShutdownError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ShutdownError {}

#[cfg(test)]
mod tests;
