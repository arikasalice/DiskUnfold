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
        let action = match request.target {
            QueryCancelTarget::Request { request_seq } => {
                self.0
                    .registry
                    .cancel_request(&self.0.owner, &request.region_ref, request_seq)
            }
            QueryCancelTarget::Job { job_ref } => {
                self.0
                    .registry
                    .cancel_job(&self.0.owner, &request.region_ref, &job_ref)
            }
        }
        .map_err(region_error)?;
        let outcome = match action {
            CancelAction::Accepted => QueryCancelOutcome::Accepted,
            CancelAction::NoEffect => QueryCancelOutcome::NoEffect,
            CancelAction::Job(id) => self.0.cancel_job(&id)?,
        };
        Ok(QueryCancelled { outcome })
    }
}
