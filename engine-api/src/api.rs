use du_core_contracts::{CallEnvelope, ResultEnvelope};
use std::time::Duration;

use crate::{pipeline, registry::Registry, shutdown::ShutdownHooks};

pub use crate::contracts::{CallerCtx, Channel, OutboundFilter, UseCase};

#[derive(Debug, PartialEq, Eq)]
pub struct BuildError {
    pub duplicate_name: &'static str,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "duplicate use case: {}", self.duplicate_name)
    }
}

impl std::error::Error for BuildError {}

#[derive(Default)]
pub struct EngineBuilder {
    registry: Registry,
    duplicate_name: Option<&'static str>,
    outbound_filter: Option<Box<dyn OutboundFilter>>,
    session_retire: Option<Box<dyn Fn() + Send + Sync>>,
    shutdown_hooks: Option<ShutdownHooks>,
}

impl EngineBuilder {
    pub fn register<U: UseCase>(&mut self, use_case: U) {
        if !self.registry.insert(use_case) && self.duplicate_name.is_none() {
            self.duplicate_name = Some(U::NAME);
        }
    }

    pub fn with_outbound_filter(&mut self, filter: impl OutboundFilter) {
        self.outbound_filter = Some(Box::new(filter));
    }

    pub(crate) fn on_session_retire(&mut self, retire: impl Fn() + Send + Sync + 'static) {
        let previous = self.session_retire.take();
        self.session_retire = Some(Box::new(move || {
            if let Some(previous) = &previous {
                previous();
            }
            retire();
        }));
    }

    /// Installs the desktop job-runtime shutdown hooks at composition time.
    pub(crate) fn on_shutdown_hooks(&mut self, hooks: ShutdownHooks) {
        if self.shutdown_hooks.is_some() {
            if self.duplicate_name.is_none() {
                self.duplicate_name = Some("engine.shutdown");
            }
        } else {
            self.shutdown_hooks = Some(hooks);
        }
    }

    pub fn build(self) -> Result<Engine, BuildError> {
        if let Some(duplicate_name) = self.duplicate_name {
            return Err(BuildError { duplicate_name });
        }
        Ok(Engine {
            registry: self.registry,
            outbound_filter: self.outbound_filter,
            session_retire: self.session_retire,
            shutdown_hooks: self.shutdown_hooks,
        })
    }
}

pub struct Engine {
    registry: Registry,
    outbound_filter: Option<Box<dyn OutboundFilter>>,
    session_retire: Option<Box<dyn Fn() + Send + Sync>>,
    shutdown_hooks: Option<ShutdownHooks>,
}

impl Engine {
    /// Retires query regions when the owning desktop window session ends.
    pub fn retire_window_session(&self) {
        if let Some(retire) = &self.session_retire {
            retire();
        }
    }

    /// Atomically confirms the active `change.*` job count and closes job admission if allowed.
    ///
    /// Call this before MCP access closure. `ConfirmRequired` leaves the engine
    /// open; `Accepted` prevents new jobs while the caller closes gateway access.
    pub fn close_job_admission(
        &self,
        confirmed: bool,
    ) -> Result<crate::shutdown::ShutdownAdmission, crate::shutdown::ShutdownError> {
        self.shutdown_hooks.as_ref().map_or(
            Ok(crate::shutdown::ShutdownAdmission::Accepted {
                active_change_jobs: 0,
            }),
            |hooks| hooks.close_admission(confirmed),
        )
    }

    /// Requests F24 cancellation for all active jobs after accepted shutdown admission.
    ///
    /// Call [`Engine::close_job_admission`] first and proceed only after it returns
    /// `Accepted` or `AlreadyClosing`. This call does not wait for job bodies or
    /// their history writes to finish.
    /// The caller must observe their terminal events and wait for the history
    /// delivery barrier before closing the database.
    pub fn prepare_shutdown(
        &self,
    ) -> Result<crate::shutdown::ShutdownPreparation, crate::shutdown::ShutdownError> {
        self.shutdown_hooks.as_ref().map_or_else(
            || Ok(crate::shutdown::ShutdownPreparation::no_runtime()),
            ShutdownHooks::prepare,
        )
    }

    /// Starts the F24 delivery barrier after all job bodies have become terminal.
    ///
    /// Poll the returned ticket until `Complete` before stopping workers or
    /// releasing the engine's history owner. A failed or timed-out barrier does
    /// not authorize database closure.
    pub fn begin_history_commit_barrier(
        &self,
        timeout: Duration,
    ) -> Result<crate::shutdown::HistoryCommitBarrier, crate::shutdown::ShutdownError> {
        self.shutdown_hooks.as_ref().map_or_else(
            || Ok(crate::shutdown::HistoryCommitBarrier::no_runtime()),
            |hooks| hooks.begin_history_barrier(timeout),
        )
    }

    /// Stops and joins the desktop job-worker threads after all work has drained.
    pub fn close_job_workers(&self) -> Result<(), crate::shutdown::ShutdownError> {
        self.shutdown_hooks
            .as_ref()
            .map_or(Ok(()), ShutdownHooks::close_workers)
    }

    pub fn call(&self, envelope: CallEnvelope, caller: &CallerCtx) -> ResultEnvelope {
        pipeline::call(
            &self.registry,
            self.outbound_filter.as_deref(),
            envelope,
            caller,
        )
    }
}
