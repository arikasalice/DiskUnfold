//! Production adapters for the app-host shutdown coordinator.

use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Timestamp,
    shutdown_ipc::{AppLifecycleChanged, AppLifecycleState},
};
use du_engine_api::{
    api::Engine,
    desktop::DesktopEvents,
    shutdown::{ActiveJobCounts, HistoryCommitBarrier, HistoryCommitStatus, TerminalHistoryFlush},
};
use du_mcp_gateway::access::AccessGate;

use super::{
    GatewayCloseReport, LifecycleEvent, LifecycleStatus, ListenerClose, QuitAdmission,
    ShutdownPorts,
};

pub(crate) use super::controller::{QuitHandler, ShutdownController};

/// A narrow app-host port for closing the concrete MCP listener, when installed.
pub(crate) trait GatewayListenerCloser: Send + Sync {
    /// Closes the acceptor and every accepted socket before returning `Closed`.
    /// The implementation must return by `deadline`; the host reports a late
    /// return as a timeout.
    fn close_listener(&self, deadline: Instant) -> Result<ListenerClose, String>;
}

/// The F12 listener is not present in this composition; close only the access gate.
///
/// This adapter reports `NotInstalled` and makes no claim that a socket closed.
pub(crate) struct NoListenerCloser;

impl GatewayListenerCloser for NoListenerCloser {
    fn close_listener(&self, _: Instant) -> Result<ListenerClose, String> {
        Ok(ListenerClose::NotInstalled)
    }
}

/// The MCP gateway pieces closed by IF-12.
pub(crate) struct GatewayResources {
    pub gate: AccessGate,
    pub listener: Arc<dyn GatewayListenerCloser>,
}

/// Owners released in the storage steps, after the engine.
pub(crate) struct StorageResources {
    pub database: Arc<Mutex<Option<crate::shared_database::SharedDatabase>>>,
    /// Releases platform-specific engine-adjacent owners (for example the recent-scope worker).
    pub release_extra_owners: Box<dyn Fn() + Send + Sync>,
}

/// Production shutdown ports backed by app-host's shared engine and storage owners.
pub(crate) struct HostShutdownResources {
    pub engine: Arc<Mutex<Option<Arc<Engine>>>>,
    pub gateway: GatewayResources,
    pub events: Arc<DesktopEvents>,
    pub storage: StorageResources,
}

pub(crate) struct HostShutdownPorts {
    engine: Arc<Mutex<Option<Arc<Engine>>>>,
    gateway: GatewayResources,
    events: Arc<DesktopEvents>,
    storage: StorageResources,
    history_barrier: Option<HistoryCommitBarrier>,
    history_failures: Option<usize>,
    terminal_flush: Option<TerminalHistoryFlush>,
    /// Sessions closed and responses discarded by the gate's close, kept even when
    /// the listener close failed and no report was returned.
    gateway_counts: (usize, usize),
}

impl HostShutdownPorts {
    /// Builds the host adapter around its injected engine, gateway, and storage owners.
    pub(crate) fn new(resources: HostShutdownResources) -> Self {
        Self {
            engine: resources.engine,
            gateway: resources.gateway,
            events: resources.events,
            storage: resources.storage,
            history_barrier: None,
            history_failures: None,
            terminal_flush: None,
            gateway_counts: (0, 0),
        }
    }

    fn with_engine<T>(
        &self,
        operation: impl FnOnce(&Engine) -> Result<T, String>,
    ) -> Result<T, String> {
        let slot = self
            .engine
            .lock()
            .map_err(|_| "engine owner lock is poisoned".to_owned())?;
        let engine = slot
            .as_ref()
            .ok_or_else(|| "engine owner was released".to_owned())?;
        operation(engine)
    }
}

impl ShutdownPorts for HostShutdownPorts {
    fn close_job_admission(&mut self, confirmed: bool) -> Result<QuitAdmission, String> {
        self.with_engine(|engine| {
            engine
                .close_job_admission(confirmed)
                .map(QuitAdmission::from)
                .map_err(|error| error.to_string())
        })
    }

    /// IF-12: closes the gate first, so no new session or request is admitted
    /// whatever the listener does, then asks the listener to close.
    fn close_mcp_access(&mut self, deadline: Instant) -> Result<GatewayCloseReport, String> {
        let counts = self.gateway.gate.close_access();
        // The gate returns its first close report on every call, so a retry reports
        // the same counts instead of zeros.
        self.gateway_counts = (counts.sessions_closed, counts.responses_discarded);
        let listener = self.gateway.listener.close_listener(deadline)?;
        let timed_out = Instant::now() > deadline;
        Ok(GatewayCloseReport {
            access_closed: !self.gateway.gate.is_accepting(),
            listener,
            timed_out,
            sessions_closed: self.gateway_counts.0,
            responses_discarded: self.gateway_counts.1,
            detail: timed_out.then(|| "the listener returned after the close deadline".to_owned()),
        })
    }

    fn gateway_close_counts(&self) -> (usize, usize) {
        self.gateway_counts
    }

    fn force_close_mcp_access(&mut self) -> bool {
        self.gateway.gate.close_access();
        !self.gateway.gate.is_accepting()
    }

    fn prepare_engine_shutdown(
        &mut self,
    ) -> Result<du_engine_api::shutdown::ShutdownPreparation, String> {
        self.with_engine(|engine| engine.prepare_shutdown().map_err(|error| error.to_string()))
    }

    fn list_active_jobs(&mut self) -> Result<Vec<du_core_contracts::Id>, String> {
        self.with_engine(|engine| {
            engine
                .list_active_jobs_for_shutdown()
                .map_err(|error| error.to_string())
        })
    }

    fn active_job_counts(&mut self) -> Result<ActiveJobCounts, String> {
        self.with_engine(|engine| {
            engine
                .active_job_counts()
                .map_err(|error| error.to_string())
        })
    }

    fn begin_terminal_history_flush(&mut self, timeout: std::time::Duration) -> Result<(), String> {
        let flush = self.with_engine(|engine| {
            engine
                .begin_terminal_history_flush(timeout)
                .map_err(|error| error.to_string())
        })?;
        self.terminal_flush = Some(flush);
        Ok(())
    }

    fn poll_terminal_history_flush(&mut self) -> HistoryCommitStatus {
        self.terminal_flush
            .as_ref()
            .map_or(HistoryCommitStatus::Failed, TerminalHistoryFlush::poll)
    }

    fn begin_history_commit_barrier(&mut self, timeout: std::time::Duration) -> Result<(), String> {
        let barrier = self.with_engine(|engine| {
            engine
                .begin_history_commit_barrier(timeout)
                .map_err(|error| error.to_string())
        })?;
        self.history_barrier = Some(barrier);
        Ok(())
    }

    fn poll_history_commit_barrier(&mut self) -> HistoryCommitStatus {
        let Some(barrier) = &self.history_barrier else {
            return HistoryCommitStatus::Failed;
        };
        let status = barrier.poll();
        if status == HistoryCommitStatus::Failed {
            self.history_failures = barrier.history_failures();
        }
        status
    }

    fn history_failure_count(&self) -> Option<usize> {
        self.history_failures
    }

    fn close_job_workers(&mut self) -> Result<(), String> {
        self.with_engine(|engine| {
            engine
                .close_job_workers()
                .map_err(|error| error.to_string())
        })
    }

    /// Retires the window session on the orderly path, then drops the engine owner.
    fn release_engine(&mut self) {
        if let Ok(mut engine) = self.engine.lock()
            && let Some(engine) = engine.take()
        {
            engine.retire_window_session();
        }
        (self.storage.release_extra_owners)();
    }

    fn checkpoint_and_close_database(&mut self) -> Result<(), String> {
        let database = self
            .storage
            .database
            .lock()
            .map_err(|_| "database owner lock is poisoned".to_owned())?
            .take()
            .ok_or_else(|| "database owner is unavailable".to_owned())?;
        match database.close() {
            Ok(_) => Ok(()),
            Err(error) => {
                let detail = error.to_string();
                let database = error.into_database();
                let mut owner = self
                    .storage
                    .database
                    .lock()
                    .map_err(|_| "database owner lock is poisoned".to_owned())?;
                *owner = Some(database);
                Err(detail)
            }
        }
    }

    fn zeroize_secrets(&mut self) -> Result<(), String> {
        // No MCP credential store is installed in this branch; F12 owns it.
        Ok(())
    }

    fn publish_lifecycle(&mut self, event: LifecycleEvent) {
        let Some(observed_at) = Timestamp::from_unix_millis(event.observed_at_ms) else {
            return;
        };
        let state = lifecycle_state(event.status);
        let previous_state = lifecycle_state(event.previous);
        let Ok(body) = serde_json::to_value(AppLifecycleChanged {
            state,
            previous_state,
            remaining_job_count: u64::try_from(event.remaining_tasks).unwrap_or(u64::MAX),
            state_sequence: event.sequence,
            observed_at: observed_at.clone(),
            confirmation_required: event.confirmation_needed.map(|_| true),
            in_progress_change_count: event
                .confirmation_needed
                .map(|count| u64::try_from(count).unwrap_or(u64::MAX)),
        }) else {
            return;
        };
        self.events.publish_host_event(EventEnvelope {
            topic: "app.lifecycle_changed".to_owned(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id: None,
            job_ref: None,
            result_version: None,
            occurred_at: observed_at,
            body,
        });
    }

    fn final_events_flushed(&self) -> bool {
        !self.events.has_pending_events()
    }
}

fn lifecycle_state(status: LifecycleStatus) -> AppLifecycleState {
    match status {
        LifecycleStatus::Starting => AppLifecycleState::Starting,
        LifecycleStatus::Running => AppLifecycleState::Running,
        LifecycleStatus::AccessClosed => AppLifecycleState::AccessClosed,
        LifecycleStatus::Draining => AppLifecycleState::Draining,
        LifecycleStatus::Closed => AppLifecycleState::Closed,
    }
}

pub(crate) use super::types::current_time_ms;
