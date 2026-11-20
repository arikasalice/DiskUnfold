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
    let (scope_first_root, scope_root_count) =
        scope_roots(row.scope_first_root, row.scope_root_count);
    AnalysisJobListRow {
        job_ref: row.job_id,
        kind: snapshot.kind_id,
        scope_summary: row.scope_summary,
        scope_first_root,
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

/// Projects the first root and root count, both or neither.
///
/// A root follows the history's root text rule (1 to 32,768 characters); an
/// empty or longer first root omits both fields, as the history list does.
fn scope_roots(first: Option<String>, count: usize) -> (Option<String>, Option<u32>) {
    match first {
        Some(root) if !root.is_empty() && root.chars().count() <= 32_768 => {
            (Some(root), Some(u32::try_from(count).unwrap_or(u32::MAX)))
        }
        _ => (None, None),
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

#[cfg(test)]
mod tests {
    use super::scope_roots;

    #[test]
    fn scope_roots_follow_the_history_root_text_rule() {
        assert_eq!(scope_roots(None, 0), (None, None));
        assert_eq!(scope_roots(Some(String::new()), 1), (None, None));
        let longest = "a".repeat(32_768);
        assert_eq!(
            scope_roots(Some(longest.clone()), 2),
            (Some(longest), Some(2))
        );
        assert_eq!(scope_roots(Some("a".repeat(32_769)), 1), (None, None));
        assert_eq!(
            scope_roots(Some("/root".into()), usize::MAX),
            (Some("/root".into()), Some(u32::MAX))
        );
    }
}
