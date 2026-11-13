//! Bounded retry, forced exit and diagnostics for the shutdown coordinator.

use std::time::Instant;

use super::{LifecycleStatus, ShutdownCoordinator, ShutdownError, ShutdownOutcome, ShutdownPorts};

/// Diagnostics kept for inspection; older entries are dropped first.
const MAX_DIAGNOSTICS: usize = 32;

impl<P: ShutdownPorts> ShutdownCoordinator<P> {
    /// Runs one UI-thread tick: rebuilds the job set, then advances one step.
    ///
    /// Failures are logged and counted. After `phase_retries` consecutive
    /// failures the exit is forced: the terminal `closed` event is published and
    /// the outcome becomes a failure, so the process leaves with a nonzero code
    /// instead of retrying forever.
    pub(crate) fn step(&mut self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        let result = self
            .resync_active_jobs(observed_at_ms)
            .and_then(|()| self.advance(observed_at_ms));
        let Err(error) = result else {
            self.consecutive_failures = 0;
            return Ok(());
        };
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.note(format!(
            "shutdown step failed in phase {} (attempt {}): {error}",
            self.phase.name(),
            self.consecutive_failures
        ));
        if self.consecutive_failures < self.timeouts.phase_retries {
            return Err(error);
        }
        let exhausted = ShutdownError::RetriesExhausted {
            phase: self.phase.name(),
            last: error.to_string(),
        };
        self.abort(exhausted.to_string(), observed_at_ms);
        Err(exhausted)
    }

    /// Ends the shutdown now. Publishes the terminal `closed` event so the window
    /// is told, and records the failure. Storage is left for next-start recovery.
    pub(crate) fn abort(&mut self, reason: String, observed_at_ms: i64) {
        if self.is_closed() {
            return;
        }
        self.note(format!("forcing exit: {reason}"));
        self.outcome = ShutdownOutcome::Forced(reason);
        self.phase = super::Phase::Closed;
        self.closed_at = Some(Instant::now());
        if self.emit(LifecycleStatus::Closed, observed_at_ms).is_err() {
            self.note("terminal closed event could not be numbered".to_owned());
        }
    }

    /// True once the shutdown is closed and the final event has left the host
    /// queue, or the flush bound has passed so a missing page cannot hold the exit.
    pub(crate) fn may_exit(&self) -> bool {
        self.is_closed()
            && (self.ports.final_events_flushed()
                || self
                    .closed_at
                    .is_none_or(|closed| closed.elapsed() >= self.timeouts.final_flush))
    }

    pub(crate) fn note(&mut self, message: String) {
        eprintln!("DiskUnfold shutdown: {message}");
        if self.diagnostics.len() == MAX_DIAGNOSTICS {
            self.diagnostics.remove(0);
        }
        self.diagnostics.push(message);
    }

    #[cfg(test)]
    pub(crate) fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    #[cfg(test)]
    pub(crate) fn gateway_report(&self) -> Option<&super::GatewayCloseReport> {
        self.gateway.as_ref()
    }
}
