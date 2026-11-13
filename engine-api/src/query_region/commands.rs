//! GUI query-region command composition for one desktop window session.
use super::{CancelAction, Owner, RegionError, RegionRegistry};
use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};
use du_core_contracts::{
    Id,
    ports::Random,
    query_region_ipc::{
        QueryCancelOutcome, QueryCancelRequest, QueryCancelTarget, QueryCancelled,
        QueryRegionCloseRequest, QueryRegionClosed, QueryRegionKind, QueryRegionOpenRequest,
        QueryRegionOpened,
    },
};
use du_job_runtime::{CancelResult, JobIdentity, JobRuntime, RuntimeError};
use std::sync::Arc;

pub(crate) struct Regions {
    pub registry: RegionRegistry<QueryRegionKind>,
    pub owner: Owner,
    runtime: Arc<JobRuntime>,
}
impl Regions {
    pub fn register(
        builder: &mut EngineBuilder,
        ids: Arc<dyn JobIdentity>,
        random: &dyn Random,
        runtime: Arc<JobRuntime>,
    ) -> Result<Arc<Self>, RuntimeError> {
        let owner = Owner {
            session: ids.next_id(),
            window: ids.next_id(),
        };
        let registry = RegionRegistry::new(owner.clone(), 1024, ids, random)
            .map_err(|_| RuntimeError::InvalidState)?;
        let state = Arc::new(Self {
            registry,
            owner,
            runtime,
        });
        let retiring = state.clone();
        builder.on_session_retire(move || retiring.retire());
        builder.register(Open(state.clone()));
        builder.register(Close(state.clone()));
        builder.register(Cancel(state.clone()));
        Ok(state)
    }
    fn retire(&self) {
        if let Ok(jobs) = self.registry.close_session(&self.owner) {
            for job in jobs {
                let _ = self.runtime.cancel(&job);
            }
        }
    }
    pub fn cancel_job(&self, job: &Id) -> Result<QueryCancelOutcome, EngineError> {
        match self.runtime.cancel(job) {
            Ok(CancelResult::Accepted(_)) => Ok(QueryCancelOutcome::Accepted),
            Ok(CancelResult::NoEffect(_)) => Ok(QueryCancelOutcome::NoEffect),
            Ok(CancelResult::AlreadyTerminal(_)) => Ok(QueryCancelOutcome::AlreadyTerminal),
            Ok(CancelResult::Unsupported(_)) => Err(EngineError::reason("support.internal_error")),
            Err(RuntimeError::NotFound) => Err(EngineError::reason("ref.unavailable")),
            Err(_) => Err(EngineError::reason("support.internal_error")),
        }
    }
    fn cancel_bound_job(
        &self,
        region_ref: &Id,
        request_seq: u32,
        job_ref: &Id,
    ) -> Result<QueryCancelOutcome, EngineError> {
        if self
            .registry
            .current_job_for_request(&self.owner, region_ref, request_seq)
            .map_err(region_error)?
            .as_ref()
            != Some(job_ref)
        {
            return Err(EngineError::reason("ref.unavailable"));
        }
        #[cfg(test)]
        crate::search::test_gate::pause_cancel_preflight(job_ref)
            .map_err(|_| EngineError::reason("support.internal_error"))?;
        let cancelled = self
            .runtime
            .cancel_query_if(job_ref, |state| {
                self.registry
                    .detach_job_if_current(
                        &self.owner,
                        region_ref,
                        request_seq,
                        job_ref,
                        !state.is_terminal(),
                    )
                    .map_err(region_runtime_error)
            })
            .map_err(runtime_error)?;
        let Some((result, _)) = cancelled else {
            return Err(EngineError::reason("ref.unavailable"));
        };
        match result {
            CancelResult::Accepted(_) => Ok(QueryCancelOutcome::Accepted),
            CancelResult::NoEffect(_) => Ok(QueryCancelOutcome::NoEffect),
            CancelResult::AlreadyTerminal(_) => Ok(QueryCancelOutcome::AlreadyTerminal),
            CancelResult::Unsupported(_) => Err(EngineError::reason("support.internal_error")),
        }
    }
}
impl Drop for Regions {
    fn drop(&mut self) {
        self.retire();
    }
}
pub(crate) fn region_error(error: RegionError) -> EngineError {
    EngineError::reason(match error {
        RegionError::WrongKind | RegionError::InvalidSequence => "command.body_invalid",
        RegionError::Capacity => "query.busy",
        RegionError::Unavailable => "ref.unavailable",
        _ => "support.internal_error",
    })
}
struct Open(Arc<Regions>);
impl UseCase for Open {
    const NAME: &'static str = "query.region.open";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = QueryRegionOpenRequest;
    type Response = QueryRegionOpened;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        Ok(QueryRegionOpened {
            region_ref: self
                .0
                .registry
                .open(&self.0.owner, request.kind)
                .map_err(region_error)?,
        })
    }
}
struct Close(Arc<Regions>);
impl UseCase for Close {
    const NAME: &'static str = "query.region.close";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = QueryRegionCloseRequest;
    type Response = QueryRegionClosed;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        if let Some(job) = self
            .0
            .registry
            .close(&self.0.owner, &request.region_ref)
            .map_err(region_error)?
        {
            self.0.cancel_job(&job)?;
        }
        Ok(QueryRegionClosed {})
    }
}
struct Cancel(Arc<Regions>);
impl UseCase for Cancel {
    const NAME: &'static str = "query.cancel";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = QueryCancelRequest;
    type Response = QueryCancelled;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let outcome = match request.target {
            QueryCancelTarget::Request { request_seq } => {
                #[cfg(test)]
                crate::search::test_gate::pause_request_cancel(&request.region_ref)
                    .map_err(|_| EngineError::reason("support.internal_error"))?;
                match self
                    .0
                    .registry
                    .cancel_unbound_request(&self.0.owner, &request.region_ref, request_seq)
                    .map_err(region_error)?
                {
                    CancelAction::Accepted => QueryCancelOutcome::Accepted,
                    CancelAction::NoEffect => QueryCancelOutcome::NoEffect,
                    CancelAction::Job(job_ref) => {
                        self.0
                            .cancel_bound_job(&request.region_ref, request_seq, &job_ref)?
                    }
                }
            }
            QueryCancelTarget::Job { job_ref } => {
                let request_seq = self
                    .0
                    .registry
                    .current_sequence_for_job(&self.0.owner, &request.region_ref, &job_ref)
                    .map_err(region_error)?;
                self.0
                    .cancel_bound_job(&request.region_ref, request_seq, &job_ref)?
            }
        };
        Ok(QueryCancelled { outcome })
    }
}

fn region_runtime_error(error: RegionError) -> RuntimeError {
    match error {
        RegionError::Poisoned => RuntimeError::Poisoned,
        _ => RuntimeError::InvalidState,
    }
}

fn runtime_error(error: RuntimeError) -> EngineError {
    EngineError::reason(match error {
        RuntimeError::NotFound => "ref.unavailable",
        _ => "support.internal_error",
    })
}
