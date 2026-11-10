//! Job query and control use cases for the desktop analysis flow.
use du_analysis_index::IngestSink;
use std::sync::Arc;

use du_core_contracts::analysis_ipc::{
    AnalysisJobControl, AnalysisJobRequest, AnalysisJobStatus, JobControlOutcome,
};
use du_job_runtime::{CancelResult, JobRuntime, RuntimeError};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

/// Registers job status and cooperative cancellation for the composed GUI runtime.
pub fn register(builder: &mut EngineBuilder, runtime: Arc<JobRuntime>) {
    register_inner(builder, runtime, None);
}

/// Registers job queries with access to the desktop scan result store.
pub fn register_with_scan_store(
    builder: &mut EngineBuilder,
    runtime: Arc<JobRuntime>,
    store: IngestSink,
) {
    register_inner(builder, runtime, Some(store));
}

fn register_inner(
    builder: &mut EngineBuilder,
    runtime: Arc<JobRuntime>,
    store: Option<IngestSink>,
) {
    builder.register(Status {
        runtime: runtime.clone(),
        store,
    });
    builder.register(Cancel(runtime));
}

struct Status {
    runtime: Arc<JobRuntime>,
    store: Option<IngestSink>,
}

impl UseCase for Status {
    const NAME: &'static str = "job.status";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = AnalysisJobRequest;
    type Response = AnalysisJobStatus;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let snapshot = self
            .runtime
            .snapshot(&request.job_ref)
            .map_err(runtime_error)?;
        let scan_result = match (
            &self.store,
            &snapshot.result_binding,
            snapshot.kind_id.as_str(),
        ) {
            (Some(store), Some(binding), "scan.metadata") => Some(
                store
                    .scan_result(&binding.lineage_id, &binding.scope_version)
                    .map_err(|_| EngineError::reason("support.internal_error"))?,
            ),
            _ => None,
        };
        Ok(AnalysisJobStatus {
            job_ref: request.job_ref,
            kind: snapshot.kind_id,
            state: snapshot.state,
            sequence: snapshot.sequence,
            result_version: snapshot.result_version_id,
            reason_key: snapshot.failure_reason_key,
            scan_result,
        })
    }
}

struct Cancel(Arc<JobRuntime>);

impl UseCase for Cancel {
    const NAME: &'static str = "job.cancel";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = AnalysisJobRequest;
    type Response = AnalysisJobControl;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let (result, sequence) = self
            .0
            .cancel_with_sequence(&request.job_ref)
            .map_err(runtime_error)?;
        use JobControlOutcome::{Accepted, AlreadyTerminal, NoEffect, Unsupported};
        let (outcome, state, reason_key) = match result {
            CancelResult::Accepted(state) => (Accepted, state, None),
            CancelResult::NoEffect(state) => (NoEffect, state, None),
            CancelResult::Unsupported(state) => {
                (Unsupported, state, Some("job.control_unsupported".into()))
            }
            CancelResult::AlreadyTerminal(state) => (AlreadyTerminal, state, None),
        };
        Ok(AnalysisJobControl {
            outcome,
            state,
            reason_key,
            sequence,
        })
    }
}

fn runtime_error(error: RuntimeError) -> EngineError {
    EngineError::reason(match error {
        RuntimeError::NotFound => "job.not_found",
        _ => "support.internal_error",
    })
}
