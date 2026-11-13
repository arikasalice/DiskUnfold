//! Ordered app-host shutdown orchestration.
//!
//! The lifecycle owner connects this coordinator to the window, gateway access
//! gate, engine, and final database owner. Every bounded wait in the sequence
//! ends in a defined state: a step that keeps failing forces a nonzero exit, a
//! gateway failure closes the gate and moves on, and jobs that outlive the drain
//! bound are abandoned without a success record.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use du_core_contracts::Id;
use du_engine_api::shutdown::{HistoryCommitStatus, ShutdownPreparation, WatchStopResult};

mod abandon;
mod controller;
pub(crate) mod host;
mod retry;
mod types;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
pub(crate) mod webview;

use abandon::AbandonFlush;
pub(crate) use types::*;

/// Bounds for every wait in the shutdown sequence.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShutdownTimeouts {
    /// Longest wait for cancelled jobs to reach a terminal state.
    pub drain: Duration,
    /// Longest wait for bodies to return from their current OS call once the
    /// user chose to leave without waiting.
    pub os_return: Duration,
    /// IF-12 deadline for the gateway close.
    pub gateway: Duration,
    /// Failed gateway closes tolerated before the gate is force-closed.
    pub gateway_attempts: u32,
    /// Consecutive failed ticks tolerated before the exit is forced.
    pub phase_retries: u32,
    /// Longest wait for the final lifecycle event to leave the host queue.
    pub final_flush: Duration,
    /// Longest wait for the history of finished jobs to be handed over before
    /// jobs that did not return are abandoned.
    pub abandon_flush: Duration,
}

impl Default for ShutdownTimeouts {
    fn default() -> Self {
        Self {
            drain: Duration::from_secs(30),
            os_return: Duration::from_secs(10),
            gateway: Duration::from_secs(2),
            gateway_attempts: 3,
            // About two seconds at the 16 ms shutdown tick.
            phase_retries: 125,
            final_flush: Duration::from_millis(250),
            abandon_flush: Duration::from_secs(2),
        }
    }
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

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::ClosingAccess => "closing_access",
            Self::AccessClosed => "access_closed",
            Self::Draining => "draining",
            Self::HistoryCommit => "history_commit",
            Self::ClosingStorage => "closing_storage",
            Self::ZeroizingSecrets => "zeroizing_secrets",
            Self::Closed => "closed",
        }
    }
}

/// Ordered coordinator for the shutdown sequence (F15 FR7.HD1, FR10.HD1).
///
/// F24 cancellation records terminal history for every job that reaches a
/// terminal state, so both quit modes run the ordinary history barrier for work
/// that finished. Work still running at the bound is abandoned and never
/// reported as a success.
pub(crate) struct ShutdownCoordinator<P> {
    ports: P,
    timeouts: ShutdownTimeouts,
    phase: Phase,
    mode: QuitMode,
    active_jobs: HashSet<Id>,
    preparation: Option<ShutdownPreparation>,
    history_barrier_started: bool,
    engine_released: bool,
    sequence: u64,
    visible_status: LifecycleStatus,
    gateway_failures: u32,
    gateway: Option<GatewayCloseReport>,
    drain_deadline: Option<Instant>,
    os_return_deadline: Option<Instant>,
    abandon_flush: AbandonFlush,
    outcome: ShutdownOutcome,
    closed_at: Option<Instant>,
    consecutive_failures: u32,
    diagnostics: Vec<String>,
}

impl<P: ShutdownPorts> ShutdownCoordinator<P> {
    #[cfg(test)]
    pub(crate) fn new(ports: P) -> Self {
        Self::with_timeouts(ports, ShutdownTimeouts::default())
    }

    pub(crate) fn with_timeouts(ports: P, timeouts: ShutdownTimeouts) -> Self {
        Self {
            ports,
            timeouts,
            phase: Phase::Running,
            mode: QuitMode::Waiting,
            active_jobs: HashSet::new(),
            preparation: None,
            history_barrier_started: false,
            engine_released: false,
            sequence: 0,
            visible_status: LifecycleStatus::Starting,
            gateway_failures: 0,
            gateway: None,
            drain_deadline: None,
            os_return_deadline: None,
            abandon_flush: AbandonFlush::NotStarted,
            outcome: ShutdownOutcome::Clean,
            closed_at: None,
            consecutive_failures: 0,
            diagnostics: Vec::new(),
        }
    }

    /// Publishes the initial `running` lifecycle event (IF-07).
    pub(crate) fn announce_running(&mut self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        if self.visible_status != LifecycleStatus::Starting {
            return Ok(());
        }
        self.emit(LifecycleStatus::Running, observed_at_ms)
    }

    /// Tells the window that a quit needs confirmation because change work runs.
    ///
    /// The IF-07 projection keeps the state `running`. `remainingJobCount` still
    /// counts every unfinished job; the confirmation request and the number of
    /// in-progress change jobs travel in their own optional fields.
    pub(crate) fn announce_confirmation_needed(
        &mut self,
        change_jobs: usize,
        observed_at_ms: i64,
    ) -> Result<(), ShutdownError> {
        let remaining = match self.ports.active_job_counts() {
            Ok(counts) => counts.total.max(change_jobs),
            Err(error) => {
                self.note(format!(
                    "job count unavailable for the confirmation: {error}"
                ));
                change_jobs
            }
        };
        let sequence = self.next_sequence()?;
        self.ports.publish_lifecycle(LifecycleEvent {
            status: self.visible_status,
            previous: self.visible_status,
            remaining_tasks: remaining,
            confirmation_needed: Some(change_jobs),
            sequence,
            observed_at_ms,
        });
        Ok(())
    }

    /// Atomically admits a waiting quit request, or returns confirmation without mutation.
    #[cfg(test)]
    pub(crate) fn request_waiting_quit(
        &mut self,
        confirmed: bool,
    ) -> Result<QuitAdmission, ShutdownError> {
        self.request_quit(QuitMode::Waiting, confirmed)
    }

    /// Atomically admits a quit request. `confirmed` answers the change-work
    /// confirmation; an unconfirmed request with change work leaves all state alone.
    /// Calling again while closing reports `AlreadyClosing`. Asking to leave
    /// without waiting switches an in-progress waiting shutdown to the shorter
    /// bound, but while change jobs are unfinished it needs the same confirmation
    /// as the first request: an unconfirmed request then returns `ConfirmRequired`
    /// and changes nothing.
    pub(crate) fn request_quit(
        &mut self,
        mode: QuitMode,
        confirmed: bool,
    ) -> Result<QuitAdmission, ShutdownError> {
        if self.phase != Phase::Running {
            if mode == QuitMode::WithoutWaiting {
                if !confirmed && self.change_work_may_be_running() {
                    let counts = self
                        .ports
                        .active_job_counts()
                        .map_err(ShutdownError::Admission)?;
                    if counts.change > 0 {
                        return Ok(QuitAdmission::ConfirmRequired {
                            active_change_jobs: counts.change,
                        });
                    }
                }
                self.stop_waiting();
            }
            return Ok(QuitAdmission::AlreadyClosing);
        }
        let admission = self
            .ports
            .close_job_admission(confirmed)
            .map_err(ShutdownError::Admission)?;
        match admission {
            QuitAdmission::Accepted { .. } | QuitAdmission::AlreadyClosing => {
                self.phase = Phase::ClosingAccess;
                self.mode = mode;
            }
            QuitAdmission::ConfirmRequired { .. } => {}
        }
        Ok(admission)
    }

    /// Change jobs can only be unfinished while the engine still owns the job runtime
    /// and the drain has not completed.
    fn change_work_may_be_running(&self) -> bool {
        !self.engine_released
            && self.mode != QuitMode::WithoutWaiting
            && matches!(
                self.phase,
                Phase::ClosingAccess | Phase::AccessClosed | Phase::Draining
            )
    }

    fn stop_waiting(&mut self) {
        if self.mode == QuitMode::WithoutWaiting || self.phase == Phase::Closed {
            return;
        }
        self.mode = QuitMode::WithoutWaiting;
        if self.phase == Phase::Draining {
            self.os_return_deadline = Some(Instant::now() + self.timeouts.os_return);
        }
    }

    /// Advances one bounded step.
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
    #[cfg(test)]
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

    #[cfg(test)]
    pub(crate) fn remaining_tasks(&self) -> usize {
        self.active_jobs.len()
    }

    /// Returns the per-job cancellation results retained from engine preparation.
    ///
    /// The caller must surface failed outcomes with their `job_ref` and must not
    /// infer that a failed cancellation means the job is terminal. Active work is
    /// tracked separately until its terminal event or an authoritative resync.
    #[cfg(test)]
    pub(crate) fn preparation(&self) -> Option<&ShutdownPreparation> {
        self.preparation.as_ref()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.phase == Phase::Closed
    }

    pub(crate) fn is_closing(&self) -> bool {
        self.phase != Phase::Running
    }

    /// How the shutdown ended; `Clean` until it ends otherwise.
    pub(crate) fn outcome(&self) -> &ShutdownOutcome {
        &self.outcome
    }

    #[cfg(test)]
    pub(crate) fn into_ports(self) -> P {
        self.ports
    }

    fn close_access(&mut self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        let report = self.close_gateway()?;
        if report.timed_out {
            self.note(format!(
                "gateway close passed its {:?} deadline (sessions closed {}, responses discarded {})",
                self.timeouts.gateway, report.sessions_closed, report.responses_discarded
            ));
        }
        self.gateway = Some(report);
        // The count is taken before the state changes so access_closed carries it.
        self.active_jobs = self
            .ports
            .list_active_jobs()
            .map_err(ShutdownError::JobList)?
            .into_iter()
            .collect();
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
        if watch_stop == WatchStopResult::Failed {
            return Err(ShutdownError::WatcherStopUnconfirmed);
        }
        self.active_jobs = self
            .ports
            .list_active_jobs()
            .map_err(ShutdownError::JobList)?
            .into_iter()
            .collect();
        let now = Instant::now();
        self.drain_deadline = Some(now + self.timeouts.drain);
        if self.mode == QuitMode::WithoutWaiting {
            self.os_return_deadline = Some(now + self.timeouts.os_return);
        }
        self.phase = Phase::Draining;
        self.emit(LifecycleStatus::Draining, observed_at_ms)
    }

    fn begin_history_barrier(&mut self, _observed_at_ms: i64) -> Result<(), ShutdownError> {
        if !self.active_jobs.is_empty() {
            if self.drain_expired() {
                self.abandon_remaining_jobs();
            }
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
        self.closed_at = Some(Instant::now());
        self.emit(LifecycleStatus::Closed, observed_at_ms)
    }

    fn next_sequence(&mut self) -> Result<u64, ShutdownError> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(ShutdownError::SequenceExhausted)?;
        self.sequence = sequence;
        Ok(sequence)
    }

    fn emit(&mut self, status: LifecycleStatus, observed_at_ms: i64) -> Result<(), ShutdownError> {
        let sequence = self.next_sequence()?;
        let previous = self.visible_status;
        self.visible_status = status;
        self.ports.publish_lifecycle(LifecycleEvent {
            status,
            previous,
            remaining_tasks: self.active_jobs.len(),
            confirmation_needed: None,
            sequence,
            observed_at_ms,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
