//! Job query and control use cases for the desktop analysis flow.
use du_analysis_index::IngestSink;
use std::sync::Arc;

use du_core_contracts::analysis_ipc::{
    AnalysisJobControl, AnalysisJobRequest, AnalysisJobStatus, AnalysisJobTerminal,
    JobControlOutcome,
};
use du_core_contracts::job::JobSummary;
use du_job_runtime::{CancelResult, JobRuntime, RuntimeError, TerminalSnapshot};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

mod list;
mod suspension;

/// Registers job status and cooperative controls for the composed GUI runtime.
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
    builder.register(list::List(runtime.clone()));
    builder.register(suspension::Suspension::<true>(runtime.clone()));
    builder.register(suspension::Suspension::<false>(runtime.clone()));
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
        let (result_version, query_ref) = query_completion(
            &snapshot.kind_id,
            snapshot.state,
            snapshot.result_version_id,
        );
        let terminal = snapshot.terminal.map(project_terminal);
        Ok(AnalysisJobStatus {
            job_ref: request.job_ref,
            kind: snapshot.kind_id,
            state: snapshot.state,
            sequence: snapshot.sequence,
            result_version,
            query_ref,
            reason_key: snapshot.failure_reason_key,
            scan_result,
            terminal,
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
        control_response(self.0.cancel_with_sequence(&request.job_ref))
    }
}

/// Preserves the sequence and state of the acknowledgement rather than rereading the job.
fn control_response(
    result: Result<(CancelResult, u64), RuntimeError>,
) -> Result<AnalysisJobControl, EngineError> {
    let (result, sequence) = result.map_err(runtime_error)?;
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

fn runtime_error(error: RuntimeError) -> EngineError {
    EngineError::reason(match error {
        RuntimeError::NotFound => "job.not_found",
        _ => "support.internal_error",
    })
}

/// Separates the legacy query result slot at the IPC boundary from actual result versions.
pub(crate) fn query_completion(
    kind: &str,
    state: du_core_contracts::job::JobState,
    value: Option<du_core_contracts::Id>,
) -> (Option<du_core_contracts::Id>, Option<du_core_contracts::Id>) {
    if kind == "query.evaluate" {
        (
            None,
            if state == du_core_contracts::job::JobState::Done {
                value
            } else {
                None
            },
        )
    } else {
        (value, None)
    }
}

/// Projects the same locked terminal facts for event delivery and status recovery.
pub(crate) fn terminal_outcome(
    summary: JobSummary,
    sequence: u64,
    history_recorded: bool,
    warning: Option<String>,
) -> AnalysisJobTerminal {
    let (result_version, query_ref) =
        query_completion(&summary.kind_id, summary.state, summary.result_version_id);
    AnalysisJobTerminal {
        state: summary.state,
        sequence,
        completed: summary.counts.completed,
        overwritten: summary.counts.overwritten,
        failed: summary.counts.failed,
        skipped: summary.counts.skipped,
        unprocessed: summary.counts.unprocessed,
        history_recorded,
        reason_key: summary.failure_reason_key,
        unprocessed_summary: summary.unprocessed_summary,
        result_version,
        query_ref,
        warning,
    }
}

fn project_terminal(terminal: TerminalSnapshot) -> AnalysisJobTerminal {
    terminal_outcome(
        terminal.summary,
        terminal.sequence,
        terminal.history_recorded,
        terminal.warning,
    )
}
