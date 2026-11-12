//! Production adapters for the app-host shutdown coordinator.

use std::{
    sync::{Arc, Mutex},
    time::{Instant, UNIX_EPOCH},
};

use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Timestamp,
    shutdown_ipc::{
        AppLifecycleChanged, AppLifecycleState, AppQuitMode, AppQuitOutcome, AppQuitRequest,
        AppQuitResponse,
    },
};
use du_engine_api::{
    api::Engine,
    desktop::DesktopEvents,
    shutdown::{HistoryCommitBarrier, HistoryCommitStatus},
};
use du_mcp_gateway::access::AccessGate;

use super::{
    GatewayCloseReport, LifecycleEvent, LifecycleStatus, ListenerClose, QuitAdmission,
    ShutdownCoordinator, ShutdownError, ShutdownPorts,
};

/// A narrow app-host port for closing the concrete MCP listener, when installed.
pub(crate) trait GatewayListenerCloser: Send + Sync {
    /// Closes the acceptor and every accepted socket before returning `Closed`.
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

/// IPC surface shared by the dispatcher and the native close-request handlers.
pub(crate) trait QuitHandler: Send + Sync {
    fn request_quit(&self, request: &AppQuitRequest) -> Result<AppQuitResponse, ShutdownError>;
    fn is_closing(&self) -> bool;
}

/// Serializes quit admission, coordinator progress, and shutdown event publication.
pub(crate) struct ShutdownController<P> {
    coordinator: Mutex<ShutdownCoordinator<P>>,
}

impl<P: ShutdownPorts> ShutdownController<P> {
    pub(crate) fn new(ports: P) -> Self {
        Self {
            coordinator: Mutex::new(ShutdownCoordinator::new(ports)),
        }
    }

    /// Advances one bounded shutdown step and rebuilds the job set from its owner.
    pub(crate) fn pump(&self, observed_at_ms: i64) -> Result<(), ShutdownError> {
        let mut coordinator = self.coordinator.lock().map_err(|_| {
            ShutdownError::Engine("shutdown coordinator lock is poisoned".to_owned())
        })?;
        coordinator.resync_active_jobs(observed_at_ms)?;
        coordinator.advance(observed_at_ms)
    }

    pub(crate) fn should_exit(&self) -> bool {
        self.coordinator
            .lock()
            .is_ok_and(|coordinator| coordinator.is_closed())
    }

    #[cfg(test)]
    pub(crate) fn remaining_tasks(&self) -> usize {
        self.coordinator
            .lock()
            .map_or(0, |coordinator| coordinator.remaining_tasks())
    }

    #[cfg(test)]
    pub(crate) fn with_ports<R>(&self, read: impl FnOnce(&P) -> R) -> Option<R> {
        self.coordinator
            .lock()
            .ok()
            .map(|coordinator| read(&coordinator.ports))
    }

    #[cfg(test)]
    pub(crate) fn with_ports_mut<R>(&self, update: impl FnOnce(&mut P) -> R) -> Option<R> {
        self.coordinator
            .lock()
            .ok()
            .map(|mut coordinator| update(&mut coordinator.ports))
    }
}

impl<P: ShutdownPorts> QuitHandler for ShutdownController<P> {
    fn request_quit(&self, request: &AppQuitRequest) -> Result<AppQuitResponse, ShutdownError> {
        if cfg!(target_os = "macos") {
            return Err(ShutdownError::NativeLifecycleUnavailable);
        }

        if request.mode == AppQuitMode::WithoutWaiting {
            // F24 cancellation still records terminal history and joins job workers.
            // The non-waiting F15 path needs that cross-contract behavior resolved.
            return Err(ShutdownError::NonWaitingNotSupported);
        }
        let mut coordinator = self.coordinator.lock().map_err(|_| {
            ShutdownError::Admission("shutdown coordinator lock is poisoned".to_owned())
        })?;
        // Do not bypass the GUI's confirmation of unfinished change work.
        let admission = coordinator.request_waiting_quit(request.confirms_unfinished_changes())?;
        Ok(response(admission))
    }

    fn is_closing(&self) -> bool {
        self.coordinator
            .lock()
            .map_or(true, |coordinator| coordinator.is_closing())
    }
}

fn response(admission: QuitAdmission) -> AppQuitResponse {
    let (outcome, count) = match admission {
        QuitAdmission::Accepted { .. } => (AppQuitOutcome::Accepted, None),
        QuitAdmission::ConfirmRequired { active_change_jobs } => (
            AppQuitOutcome::ConfirmRequired,
            Some(u64::try_from(active_change_jobs).unwrap_or(u64::MAX)),
        ),
        QuitAdmission::AlreadyClosing => (AppQuitOutcome::AlreadyClosing, None),
    };
    AppQuitResponse {
        outcome,
        in_progress_change_count: count,
    }
}

/// Production shutdown ports backed by app-host's shared engine and storage owners.
pub(crate) struct HostShutdownResources {
    pub engine: Arc<Mutex<Option<Arc<Engine>>>>,
    pub gate: AccessGate,
    pub listener: Arc<dyn GatewayListenerCloser>,
    pub events: Arc<DesktopEvents>,
    pub database: Arc<Mutex<Option<crate::shared_database::SharedDatabase>>>,
    pub release_extra_owners: Box<dyn Fn() + Send + Sync>,
}

pub(crate) struct HostShutdownPorts {
    engine: Arc<Mutex<Option<Arc<Engine>>>>,
    gate: AccessGate,
    listener: Arc<dyn GatewayListenerCloser>,
    events: Arc<DesktopEvents>,
    database: Arc<Mutex<Option<crate::shared_database::SharedDatabase>>>,
    release_extra_owners: Box<dyn Fn() + Send + Sync>,
    history_barrier: Option<HistoryCommitBarrier>,
    history_failures: Option<usize>,
}

impl HostShutdownPorts {
    /// Builds the host adapter around its injected engine, gateway, and storage owners.
    pub(crate) fn new(resources: HostShutdownResources) -> Self {
        Self {
            engine: resources.engine,
            gate: resources.gate,
            listener: resources.listener,
            events: resources.events,
            database: resources.database,
            release_extra_owners: resources.release_extra_owners,
            history_barrier: None,
            history_failures: None,
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

    fn close_mcp_access(&mut self, deadline: Instant) -> Result<GatewayCloseReport, String> {
        self.gate.close_access();
        let listener = self.listener.close_listener(deadline)?;
        Ok(GatewayCloseReport {
            access_closed: !self.gate.is_accepting(),
            listener,
            timed_out: false,
            detail: None,
        })
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

    fn release_engine(&mut self) {
        if let Ok(mut engine) = self.engine.lock() {
            engine.take();
        }
        (self.release_extra_owners)();
    }

    fn checkpoint_and_close_database(&mut self) -> Result<(), String> {
        let database = self
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
}

fn lifecycle_state(status: LifecycleStatus) -> AppLifecycleState {
    match status {
        LifecycleStatus::Running => AppLifecycleState::Running,
        LifecycleStatus::AccessClosed => AppLifecycleState::AccessClosed,
        LifecycleStatus::Draining => AppLifecycleState::Draining,
        LifecycleStatus::Closed => AppLifecycleState::Closed,
    }
}

/// Current UTC timestamp in Unix milliseconds for the shutdown state sequence.
pub(crate) fn current_time_ms() -> i64 {
    UNIX_EPOCH
        .elapsed()
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}
