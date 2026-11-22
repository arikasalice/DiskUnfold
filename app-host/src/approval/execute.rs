//! F28 approved request admission for F11 `change.trash`.
//!
//! Design items: F11 FR1.HD2.app-host.DD2, F11 FR2.HD1.app-host.DD1,
//! F28 FR10.HD1.app-host.DD1.

use super::*;
use du_change_executor::{
    ApprovalCheckFailure, record_check_finish, record_check_start, trash::ChangeTrashJob,
};
use du_core_contracts::{
    approval_check::{
        ApprovalCheckFacts, ApprovalCheckResult, ApprovalOperationKind, ApprovalRequestKind,
    },
    change_ipc::{CHANGE_EXECUTE, ChangeExecuteRequest, ChangeExecuteResponse},
    job::JobBody,
    platform_os_change::CHANGE_TRASH,
};
use du_engine_api::desktop::{DesktopDeduplicationKey, DesktopSubmission, DesktopSubmitResult};

pub(super) struct ChangeExecute(pub(super) Arc<Runtime>);

impl UseCase for ChangeExecute {
    const NAME: &'static str = CHANGE_EXECUTE;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ChangeExecuteRequest;
    type Response = ChangeExecuteResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let source = self.0.source(request.request_version_id.as_str());
        check_request_plan_source(source).map_err(error)?;
        self.0.require_primary()?;
        let change = self
            .0
            .change
            .as_ref()
            .ok_or_else(|| error("change.execution_unavailable"))?
            .clone();

        // ApprovalPrepareService re-reads the immutable version, recomputes its hash,
        // and re-judges protection through change-planner before any job is admitted.
        let prepared = self
            .0
            .prepare
            .prepare(&request.request_version_id)
            .map_err(error)?;
        if !prepared
            .version
            .items
            .iter()
            .any(|item| item.state == PlanItemState::Included && item.action == PlanAction::Trash)
        {
            return Err(error("change.no_trash_items"));
        }
        if !verify_latest(&self.0.versions, &prepared.version)? {
            self.0
                .invalidate_one(&request.request_version_id, APPROVAL_PLAN_SUPERSEDED);
            return Err(error(APPROVAL_PLAN_SUPERSEDED));
        }

        // Validate the execution body before consuming the one-shot approval token.
        let job = ChangeTrashJob::new(
            prepared.version.clone(),
            prepared.rejudgement.clone(),
            change.ops.clone(),
            change.undo.clone(),
        )
        .map_err(error)?;
        let results = job.results();
        let job = Arc::new(job);
        let body: Arc<dyn JobBody> = job.clone();

        let _transition = self
            .0
            .transition_lock
            .lock()
            .map_err(|_| error("support.internal_error"))?;
        let session_id = self
            .0
            .registry
            .lock()
            .map_err(|_| error("support.internal_error"))?
            .session_id
            .clone()
            .ok_or_else(|| error("support.internal_error"))?;
        let check = {
            let mut registry = self
                .0
                .registry
                .lock()
                .map_err(|_| error("support.internal_error"))?;
            if let Some(entry) = registry.entries.get_mut(&request.request_version_id) {
                let confirmations = entry.record.confirmations.clone();
                du_change_executor::check_approval(
                    Some(&mut entry.record),
                    &mut entry.token,
                    &prepared.version,
                    &session_id,
                    confirmations.as_ref(),
                )
            } else {
                Err(ApprovalCheckFailure::NotIssued)
            }
        };
        let check_id =
            new_id(self.0.random.as_ref()).ok_or_else(|| error("support.internal_error"))?;
        let now = now_ms();
        let result = check
            .as_ref()
            .map(|_| ApprovalCheckResult::Valid)
            .unwrap_or_else(|failure| audit_result(*failure));
        let mut facts = ApprovalCheckFacts {
            check_id,
            job_id: None,
            request_version_id: request.request_version_id.clone(),
            request_kind: ApprovalRequestKind::Plan,
            version_hash: prepared.rejudgement.recomputed_hash,
            hash_format: prepared.version.hash_format.clone(),
            operation_kind: ApprovalOperationKind::Trash,
            destination_summary: None,
            keep_summary: None,
            check_result: result,
            // The stage-one result has no per-field mismatch details.
            mismatch_fields: "[]".into(),
            approved_at_ms: now,
            created_at_ms: now,
            correlation_id: caller.correlation_id.clone(),
        };

        if let Err(failure) = check {
            audit_rejection(&change.recorder, &facts, self.0.random.as_ref())?;
            return Err(error(approval_failure_key(failure)));
        }
        if let Err(storage) = record_start(&change.recorder, &facts) {
            self.0.mark_approval_spent(&request.request_version_id);
            return Err(storage);
        }
        let facts_for_finish = facts.clone();
        let submission = DesktopSubmission {
            kind_id: CHANGE_TRASH.into(),
            deduplication_key: Some(DesktopDeduplicationKey {
                scope: "change.plan.version".into(),
                input: request.request_version_id.as_str().into(),
                channel: "gui".into(),
                key: CHANGE_TRASH.into(),
            }),
            body,
            priority: du_engine_api::desktop::DesktopJobPriority::Normal,
            scope_summary: "change.trash".into(),
            scope_roots: Vec::new(),
            correlation_id: caller.correlation_id.clone(),
            sample: false,
        };
        let runtime = self.0.clone();
        let request_version_id = request.request_version_id.clone();
        let jobs = change.jobs.clone();
        let change_for_admission = change.clone();
        let submitted = jobs.submit(submission, move |job_ref| {
            job.bind_job_ref(job_ref.clone())
                .map_err(|_| du_engine_api::desktop::DesktopJobError::InvalidState)?;
            facts.job_id = Some(job_ref.clone());
            let mut registry = runtime
                .registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let entry = registry
                .entries
                .get_mut(&request_version_id)
                .ok_or(du_engine_api::desktop::DesktopJobError::InvalidState)?;
            if entry.record.state != ApprovalRecordState::Consumed
                || entry.state != ApprovalState::Granted
            {
                return Err(du_engine_api::desktop::DesktopJobError::InvalidState);
            }
            {
                let mut executions = change_for_admission
                    .executions
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if executions.contains_key(job_ref) {
                    return Err(du_engine_api::desktop::DesktopJobError::InvalidState);
                }
                executions.insert(
                    job_ref.clone(),
                    ChangeExecution {
                        request_version_id: request_version_id.clone(),
                        results,
                        audit_facts: facts,
                        audit_finished: false,
                    },
                );
            }
            entry.state = ApprovalState::InUse;
            registry
                .states
                .insert(request_version_id.clone(), ApprovalState::InUse);
            Ok(())
        });
        match submitted {
            Ok(DesktopSubmitResult::Created(job_ref)) => Ok(ChangeExecuteResponse {
                job_ref,
                approval_state: ApprovalState::InUse,
            }),
            Ok(DesktopSubmitResult::Existing(_)) => {
                self.0.mark_approval_spent(&request.request_version_id);
                record_finish(
                    &change.recorder,
                    &facts_for_finish,
                    self.0.random.as_ref(),
                    "registration_failed",
                );
                Err(error(
                    du_core_contracts::approval_ipc::APPROVAL_ALREADY_USED,
                ))
            }
            Err(_) => {
                self.0.mark_approval_spent(&request.request_version_id);
                record_finish(
                    &change.recorder,
                    &facts_for_finish,
                    self.0.random.as_ref(),
                    "registration_failed",
                );
                Err(error("job.registration_failed"))
            }
        }
    }
}

fn audit_result(failure: ApprovalCheckFailure) -> ApprovalCheckResult {
    match failure {
        ApprovalCheckFailure::NotIssued => ApprovalCheckResult::NotIssued,
        ApprovalCheckFailure::AlreadyUsed => ApprovalCheckResult::AlreadyUsed,
        ApprovalCheckFailure::Mismatch => ApprovalCheckResult::Mismatch,
        ApprovalCheckFailure::ConfirmationMissing => ApprovalCheckResult::ConfirmationMissing,
    }
}

fn approval_failure_key(failure: ApprovalCheckFailure) -> &'static str {
    match failure {
        ApprovalCheckFailure::NotIssued => du_core_contracts::approval_ipc::APPROVAL_NOT_ISSUED,
        ApprovalCheckFailure::AlreadyUsed => du_core_contracts::approval_ipc::APPROVAL_ALREADY_USED,
        ApprovalCheckFailure::Mismatch => du_core_contracts::approval_ipc::APPROVAL_MISMATCH,
        ApprovalCheckFailure::ConfirmationMissing => APPROVAL_CONFIRMATION_MISSING,
    }
}

fn record_start(recorder: &CheckRecorder, facts: &ApprovalCheckFacts) -> Result<(), EngineError> {
    let mut recorder = recorder
        .lock()
        .map_err(|_| error("db.storage_unavailable"))?;
    record_check_start(&mut *recorder, facts).map_err(|_| error("db.storage_unavailable"))
}

fn audit_rejection(
    recorder: &CheckRecorder,
    facts: &ApprovalCheckFacts,
    random: &dyn Random,
) -> Result<(), EngineError> {
    record_start(recorder, facts)?;
    let mut finish_facts = facts.clone();
    finish_facts.check_id = new_id(random).ok_or_else(|| error("support.internal_error"))?;
    let mut recorder = recorder
        .lock()
        .map_err(|_| error("db.storage_unavailable"))?;
    record_check_finish(&mut *recorder, &finish_facts, "rejected")
        .map_err(|_| error("db.storage_unavailable"))
}

fn record_finish(
    recorder: &CheckRecorder,
    facts: &ApprovalCheckFacts,
    random: &dyn Random,
    end_state: &str,
) {
    let mut finish_facts = facts.clone();
    let Some(check_id) = new_id(random) else {
        return;
    };
    finish_facts.check_id = check_id;
    if let Ok(mut recorder) = recorder.lock() {
        let _ = record_check_finish(&mut *recorder, &finish_facts, end_state);
    }
}
