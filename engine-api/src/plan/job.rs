//! F10 immutable-plan construction job.
use std::sync::{Arc, Condvar, Mutex};

use du_change_planner::{
    api::{PlanBuilder, PlanError, PlanRequest, PlannerPorts},
    ports::{ConfirmationChoice, PlanSourceKind, RequestChannel},
};
use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Id,
    job::{
        Checkpoint, HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobOutcome,
        ProgressUnit, ResourceClass,
    },
    plan_ipc::{
        PLAN_NOT_FOUND, PLAN_NOT_READY, PLAN_RESULT_STALE, PLAN_SOURCE_NOT_CURRENT,
        PLAN_TOO_MANY_ITEMS, PLAN_VERSION_FIXED, PlanBuildRequest,
    },
};
use du_job_runtime::{DeduplicationKey, JobPriority, Submission, SubmitResult};

use crate::{
    api::CallerCtx,
    plan::{PlanService, storage},
};

const KIND: &str = "plan.build";

pub(super) fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: KIND.into(),
        version: 1,
        display_name_key: KIND.into(),
        controls: vec![JobControl::Cancel, JobControl::SetPriority],
        safe_points: vec!["before_plan_build".into(), "before_plan_commit".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::MetadataIo],
        history: HistoryMode::None,
    }
}

pub(super) fn submit(
    service: &Arc<PlanService>,
    request: PlanBuildRequest,
    caller: &CallerCtx,
) -> Result<du_core_contracts::plan_ipc::PlanBuildResponse, &'static str> {
    let operation = {
        let drafts = service
            .drafts
            .lock()
            .map_err(|_| "support.internal_error")?;
        let draft = drafts
            .entries
            .get(&request.draft_ref)
            .ok_or_else(|| super::missing_draft(&drafts, &request.draft_ref))?;
        if draft.edit_seq != request.edit_seq {
            return Err("plan.draft_stale");
        }
        draft.operation.clone()
    };
    let _operation = operation.lock().map_err(|_| "support.internal_error")?;
    let mut drafts = service
        .drafts
        .lock()
        .map_err(|_| "support.internal_error")?;
    let missing = super::missing_draft(&drafts, &request.draft_ref);
    let Some(draft) = drafts.entries.get(&request.draft_ref) else {
        return Err(missing);
    };
    if draft.edit_seq != request.edit_seq {
        return Err("plan.draft_stale");
    }
    if let Some(build) = &draft.last_build
        && build.edit_seq == request.edit_seq
    {
        let existing_job = build.job_ref.clone();
        let fixed = draft.last_fixed.is_some();
        let active = service
            .runtime
            .state(&existing_job)
            .is_ok_and(|state| !state.is_terminal());
        if fixed || active {
            super::touch(&mut drafts, &request.draft_ref);
            return Ok(du_core_contracts::plan_ipc::PlanBuildResponse {
                job_ref: existing_job,
            });
        }
    }

    let body = Arc::new(BuildBody {
        service: service.clone(),
        draft_ref: request.draft_ref.clone(),
        edit_seq: request.edit_seq,
        source_version_id: draft.source_version_id.clone(),
        analysis: draft.analysis.clone(),
        selections: draft.selections.clone(),
        operation: operation.clone(),
        job_ref: Arc::new(JobRefSlot::default()),
        correlation_id: caller.correlation_id.clone(),
    });
    let dedupe_input = request.edit_seq.to_string();
    let submitted = service
        .runtime
        .submit(Submission {
            kind_id: KIND.into(),
            deduplication_key: Some(DeduplicationKey {
                scope: request.draft_ref.as_str().to_owned(),
                input: dedupe_input.clone(),
                channel: "gui".into(),
                key: format!("{}:{dedupe_input}", request.draft_ref.as_str()),
            }),
            body: body.clone(),
            priority: JobPriority::Normal,
            scope_summary: "plan construction".into(),
            scope_roots: Vec::new(),
            correlation_id: caller.correlation_id.clone(),
            sample: false,
        })
        .map_err(|_| "support.internal_error")?;
    let job_ref = match submitted {
        SubmitResult::Created(id) | SubmitResult::Existing(id) => id,
    };
    body.job_ref.set(job_ref.clone());
    {
        let draft = drafts
            .entries
            .get_mut(&request.draft_ref)
            .ok_or("plan.draft_stale")?;
        draft.last_build = Some(super::BuildReceipt {
            edit_seq: request.edit_seq,
            job_ref: job_ref.clone(),
        });
    }
    super::touch(&mut drafts, &request.draft_ref);
    service.workers.notify();
    Ok(du_core_contracts::plan_ipc::PlanBuildResponse { job_ref })
}

#[derive(Default)]
struct JobRefSlot {
    value: Mutex<Option<Id>>,
    ready: Condvar,
}

impl JobRefSlot {
    fn set(&self, id: Id) {
        let mut value = self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *value = Some(id);
        self.ready.notify_all();
    }

    fn wait(&self) -> Option<Id> {
        let mut value = self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while value.is_none() {
            value = self
                .ready
                .wait(value)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        value.clone()
    }
}

struct BuildBody {
    service: Arc<PlanService>,
    draft_ref: Id,
    edit_seq: u64,
    source_version_id: Id,
    analysis: Arc<dyn du_change_planner::ports::AnalysisFactsPort>,
    selections: Vec<du_change_planner::ports::PlanSelection>,
    operation: Arc<Mutex<()>>,
    job_ref: Arc<JobRefSlot>,
    correlation_id: Id,
}

impl JobBody for BuildBody {
    fn kind_id(&self) -> &str {
        KIND
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let Some(job_ref) = self.job_ref.wait() else {
            return failed("support.internal_error");
        };
        if context.checkpoint() == Checkpoint::Cancel {
            return cancelled();
        }
        let planner = PlanBuilder::standard(PlannerPorts {
            analysis: self.analysis.as_ref(),
            target_check: self.service.target_check.as_ref(),
            platform: self.service.platform.as_ref(),
            rulesets: self.service.rulesets.as_ref(),
            versions: self.service.version_reader.as_ref(),
            clock: self.service.clock.as_ref(),
            ids: self.service.ids.as_ref(),
        });
        let version = match planner.build(PlanRequest {
            source_version_id: self.source_version_id.clone(),
            source_kind: PlanSourceKind::UserSelection,
            request_channel: RequestChannel::Gui,
            selections: self.selections.clone(),
            confirmations: Vec::<ConfirmationChoice>::new(),
            parent_version_id: None,
        }) {
            Ok(version) => version,
            Err(error) => return failed(plan_error(&error)),
        };
        if context.checkpoint() == Checkpoint::Cancel {
            return cancelled();
        }
        // app-db requires the exact protection ruleset revision for a stored plan.
        // The raw RulesetRead adapter returns no snapshot until its format is validated.
        if version.ruleset.is_none() {
            return failed("plan.not_ready");
        }
        let event_body = match storage::fixed_event_body(&version, job_ref.clone())
            .and_then(|event| serde_json::to_value(event).map_err(|_| "support.internal_error"))
        {
            Ok(body) => body,
            Err(reason) => return failed(reason),
        };
        let _operation = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if context.checkpoint() == Checkpoint::Cancel {
            return cancelled();
        }
        let mut drafts = match self.service.drafts.lock() {
            Ok(drafts) => drafts,
            Err(_) => return failed("support.internal_error"),
        };
        let Some(draft) = drafts.entries.get_mut(&self.draft_ref) else {
            return failed("plan.draft_stale");
        };
        if draft.edit_seq != self.edit_seq {
            return failed("plan.draft_stale");
        }
        if let Err(reason) = storage::append(&self.service.versions, &version) {
            return failed(reason);
        }
        draft.last_fixed = Some(version.version_id.clone());
        drop(drafts);
        self.service.events.publish_host_event(EventEnvelope {
            topic: PLAN_VERSION_FIXED.into(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id: Some(self.correlation_id.clone()),
            job_ref: Some(job_ref.clone()),
            result_version: None,
            occurred_at: version.created_at.clone(),
            body: event_body,
        });
        JobOutcome {
            counts: du_core_contracts::job::JobCounts {
                completed: 1,
                ..Default::default()
            },
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

fn failed(reason: &str) -> JobOutcome {
    JobOutcome {
        counts: Default::default(),
        completed_scope: false,
        failure_reason_key: Some(reason.to_owned()),
        unprocessed_summary: Some(reason.to_owned()),
        result_version_id: None,
    }
}

fn cancelled() -> JobOutcome {
    failed("request.cancelled")
}

fn plan_error(error: &PlanError) -> &'static str {
    match error {
        PlanError::Empty => "plan.empty",
        PlanError::TooManyItems => PLAN_TOO_MANY_ITEMS,
        PlanError::SourceChannelDenied => "plan.source_channel_denied",
        PlanError::SourceNotCurrent | PlanError::InvalidSourcePath => PLAN_SOURCE_NOT_CURRENT,
        PlanError::PortFailure { boundary, .. } if boundary.starts_with("analysis") => {
            PLAN_RESULT_STALE
        }
        PlanError::ParentUnavailable => PLAN_NOT_FOUND,
        PlanError::StoredHashMismatch => "db.corrupt",
        PlanError::ClockUnavailable => PLAN_NOT_READY,
        _ => "support.internal_error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_plan_error_uses_the_source_not_current_reason() {
        assert_eq!(
            plan_error(&PlanError::SourceNotCurrent),
            PLAN_SOURCE_NOT_CURRENT
        );
    }
}
