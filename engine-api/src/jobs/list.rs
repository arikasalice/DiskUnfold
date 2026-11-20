//! Reads the job table for the desktop's resynchronization baseline.
use std::sync::Arc;

use du_core_contracts::analysis_ipc::{
    AnalysisJobListRequest, AnalysisJobListRow, AnalysisJobPriority, AnalysisJobWaitReason,
};
use du_job_runtime::{JobListEntry, JobPriority, JobRuntime, JobWaitReason};

use super::{project_terminal, runtime_error};
use crate::{
    api::{CallerCtx, Channel, UseCase},
    error::EngineError,
};

pub(super) struct List(pub(super) Arc<JobRuntime>);

impl UseCase for List {
    const NAME: &'static str = "job.list";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = AnalysisJobListRequest;
    type Response = Vec<AnalysisJobListRow>;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0
            .list(request.states.as_deref())
            .map(|rows| rows.into_iter().map(project).collect())
            .map_err(runtime_error)
    }
}

fn project(row: JobListEntry) -> AnalysisJobListRow {
    let snapshot = row.snapshot;
    let scope_root_count = row
        .scope_first_root
        .is_some()
        .then(|| u32::try_from(row.scope_root_count).unwrap_or(u32::MAX));
    AnalysisJobListRow {
        job_ref: row.job_id,
        kind: snapshot.kind_id,
        scope_summary: row.scope_summary,
        scope_first_root: row.scope_first_root,
        scope_root_count,
        state: snapshot.state,
        wait_reason: wait_reason(row.wait_reason),
        priority: match row.priority {
            JobPriority::High => AnalysisJobPriority::High,
            JobPriority::Normal => AnalysisJobPriority::Normal,
            JobPriority::Low => AnalysisJobPriority::Low,
        },
        supported_controls: row.supported_controls,
        sequence: snapshot.sequence,
        terminal: snapshot.terminal.map(project_terminal),
    }
}

/// Maps a runtime wait reason to the value shared by the job table and events.
pub(crate) fn wait_reason(reason: JobWaitReason) -> AnalysisJobWaitReason {
    match reason {
        JobWaitReason::None => AnalysisJobWaitReason::None,
        JobWaitReason::Paused => AnalysisJobWaitReason::Paused,
        JobWaitReason::QueueFull => AnalysisJobWaitReason::QueueFull,
        JobWaitReason::Unknown => AnalysisJobWaitReason::Unknown,
    }
}
