//! Abandoning jobs that did not return, after a bounded flush of finished-job history.

use std::time::Instant;

use du_engine_api::shutdown::HistoryCommitStatus;

use super::{Phase, QuitMode, ShutdownCoordinator, ShutdownOutcome, ShutdownPorts};

/// Progress of the bounded history flush that precedes abandoning jobs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AbandonFlush {
    NotStarted,
    Pending,
    /// Finished; `true` when the history of finished jobs was confirmed handed over.
    Done(bool),
}

impl<P: ShutdownPorts> ShutdownCoordinator<P> {
    pub(super) fn drain_expired(&self) -> bool {
        let now = Instant::now();
        [self.drain_deadline, self.os_return_deadline]
            .into_iter()
            .flatten()
            .any(|deadline| now >= deadline)
    }

    /// Leaves the jobs that did not return in time. Nothing can close the job
    /// workers, history or database while a body still runs, so those steps are
    /// skipped; the engine owner is released so no further command reaches it,
    /// and the next start recovers the database. The jobs are never recorded as done.
    ///
    /// Before that, the history of jobs that did finish gets one bounded flush so
    /// it is not lost with the process. If the flush cannot be confirmed, the
    /// outcome says so and the shutdown log names the possible loss.
    pub(super) fn abandon_remaining_jobs(&mut self) {
        if self.abandon_flush == AbandonFlush::NotStarted {
            self.abandon_flush = match self
                .ports
                .begin_terminal_history_flush(self.timeouts.abandon_flush)
            {
                Ok(()) => AbandonFlush::Pending,
                Err(error) => {
                    self.note(format!(
                        "history of finished jobs may be lost: the flush did not start: {error}"
                    ));
                    AbandonFlush::Done(false)
                }
            };
        }
        if self.abandon_flush == AbandonFlush::Pending {
            match self.ports.poll_terminal_history_flush() {
                HistoryCommitStatus::Pending => return,
                HistoryCommitStatus::Complete => self.abandon_flush = AbandonFlush::Done(true),
                HistoryCommitStatus::TimedOut | HistoryCommitStatus::Failed => {
                    self.note(
                        "history of finished jobs may be lost: the flush did not complete"
                            .to_owned(),
                    );
                    self.abandon_flush = AbandonFlush::Done(false);
                }
            }
        }
        let history_flushed = self.abandon_flush == AbandonFlush::Done(true);
        let jobs = self.active_jobs.len();
        self.outcome = match self.mode {
            QuitMode::WithoutWaiting => ShutdownOutcome::Abandoned {
                jobs,
                history_flushed,
            },
            QuitMode::Waiting => ShutdownOutcome::DrainTimedOut {
                jobs,
                history_flushed,
            },
        };
        self.note(format!("{jobs} job(s) did not finish and were abandoned"));
        if !self.engine_released {
            self.ports.release_engine();
            self.engine_released = true;
        }
        self.phase = Phase::ZeroizingSecrets;
    }
}
