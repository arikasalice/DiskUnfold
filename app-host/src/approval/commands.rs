//! F28 review, grant and revoke use cases.

use super::*;

pub(super) struct Review(pub(super) Arc<Runtime>);
impl UseCase for Review {
    const NAME: &'static str = APPROVAL_REVIEW;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ApprovalReviewRequest;
    type Response = ApprovalReviewResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let source = self.0.source(request.request_version_id.as_str());
        check_request_plan_source(source).map_err(error)?;
        self.0.require_primary()?;
        if request.request_kind != RequestKind::Plan {
            return Err(error("approval.review_unavailable"));
        }
        let prepared = self
            .0
            .prepare
            .prepare(&request.request_version_id)
            .map_err(error)?;
        let (summary, hash, possible, reasons) =
            review_summary(&prepared.version, &prepared.rejudgement);
        self.0
            .invalidate_ruleset_changes(prepared.rejudgement.ruleset_version);
        let state = self
            .0
            .registry
            .lock()
            .map_err(|_| error("support.internal_error"))?
            .states
            .get(&request.request_version_id)
            .copied();
        Ok(ApprovalReviewResponse {
            request_version_id: request.request_version_id,
            summary,
            plan_hash: hash,
            approval_possible: possible,
            unavailable_reason_keys: (!reasons.is_empty()).then_some(reasons),
            current_approval_state: state,
        })
    }
}

pub(super) struct Grant(pub(super) Arc<Runtime>);
impl UseCase for Grant {
    const NAME: &'static str = APPROVAL_GRANT;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ApprovalGrantRequest;
    type Response = ApprovalGrantResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let source = self.0.source(request.request_version_id.as_str());
        check_request_plan_source(source).map_err(error)?;
        self.0.require_primary()?;
        let prepared = self
            .0
            .prepare
            .prepare(&request.request_version_id)
            .map_err(error)?;
        let (summary, hash, possible, reasons) =
            review_summary(&prepared.version, &prepared.rejudgement);
        if !possible {
            return Err(error(reasons.first().map_or(
                APPROVAL_REVIEW_UNAVAILABLE,
                |reason| match reason.as_str() {
                    APPROVAL_PROTECTION_DENIED => APPROVAL_PROTECTION_DENIED,
                    APPROVAL_PROTECTION_UNDETERMINED => APPROVAL_PROTECTION_UNDETERMINED,
                    APPROVAL_REVIEW_UNAVAILABLE => APPROVAL_REVIEW_UNAVAILABLE,
                    _ => APPROVAL_REVIEW_UNAVAILABLE,
                },
            )));
        }
        if request.presented_plan_hash.as_str() != hash.as_str() {
            return Err(error(APPROVAL_STALE));
        }
        if !verify_latest(&self.0.versions, &prepared.version)? {
            self.0
                .invalidate_one(&request.request_version_id, APPROVAL_PLAN_SUPERSEDED);
            return Err(error(APPROVAL_PLAN_SUPERSEDED));
        }
        validate_confirmations(&prepared.version, &summary, request.confirmations.as_ref())?;

        {
            let registry = self
                .0
                .registry
                .lock()
                .map_err(|_| error("support.internal_error"))?;
            if let Some(existing) = registry.entries.get(&request.request_version_id)
                && existing.state == ApprovalState::Granted
                && existing.record.plan_hash == prepared.rejudgement.recomputed_hash
            {
                return Ok(ApprovalGrantResponse {
                    state: ApprovalState::Granted,
                });
            }
        }

        let mut id_bytes = [0; 16];
        let mut secret = [0; 32];
        if self.0.random.fill(&mut id_bytes).is_err() {
            id_bytes.fill(0);
            return Err(error("support.internal_error"));
        }
        if self.0.random.fill(&mut secret).is_err() {
            id_bytes.fill(0);
            secret.fill(0);
            return Err(error("support.internal_error"));
        }
        let now = now_ms();
        let id = uuid_v7(id_bytes, now);
        let session_id = self
            .0
            .registry
            .lock()
            .map_err(|_| error("support.internal_error"))?
            .session_id
            .clone()
            .ok_or_else(|| error("support.internal_error"))?;
        let record = ApprovalRecord {
            approval_id: id,
            request_version_id: request.request_version_id.clone(),
            plan_hash: prepared.rejudgement.recomputed_hash,
            hash_format: prepared.version.hash_format.clone(),
            app_session_id: session_id.clone(),
            secret,
            approved_plan: prepared.version,
            confirmations: request.confirmations,
            ruleset_version: prepared.rejudgement.ruleset_version,
            state: ApprovalRecordState::Issued,
        };
        let token =
            ApprovalExecutionToken::issue(request.request_version_id.clone(), session_id, secret);
        id_bytes.fill(0);
        secret.fill(0);
        let _transition = self
            .0
            .transition_lock
            .lock()
            .map_err(|_| error("support.internal_error"))?;
        let sequence = {
            let mut registry = self
                .0
                .registry
                .lock()
                .map_err(|_| error("support.internal_error"))?;
            if let Some(existing) = registry.entries.get(&request.request_version_id)
                && existing.state == ApprovalState::Granted
                && existing.record.plan_hash == record.plan_hash
            {
                return Ok(ApprovalGrantResponse {
                    state: ApprovalState::Granted,
                });
            }
            let sequence = registry.sequence.saturating_add(1);
            registry.entries.insert(
                request.request_version_id.clone(),
                Entry {
                    record,
                    token,
                    state: ApprovalState::Granted,
                },
            );
            registry
                .states
                .insert(request.request_version_id.clone(), ApprovalState::Granted);
            registry.sequence = sequence;
            sequence
        };
        self.0.publish(
            &request.request_version_id,
            ApprovalEventState::Granted,
            None,
            sequence,
        );
        Ok(ApprovalGrantResponse {
            state: ApprovalState::Granted,
        })
    }
}

pub(super) struct Revoke(pub(super) Arc<Runtime>);
impl UseCase for Revoke {
    const NAME: &'static str = APPROVAL_REVOKE;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ApprovalRevokeRequest;
    type Response = ApprovalRevokeResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let source = self.0.source(request.request_version_id.as_str());
        check_request_plan_source(source).map_err(error)?;
        self.0.require_primary()?;
        let _transition = self
            .0
            .transition_lock
            .lock()
            .map_err(|_| error("support.internal_error"))?;
        let sequence = {
            let mut registry = self
                .0
                .registry
                .lock()
                .map_err(|_| error("support.internal_error"))?;
            let Some(entry) = registry.entries.get_mut(&request.request_version_id) else {
                return Err(error("approval.not_issued"));
            };
            if entry.state == ApprovalState::InUse {
                return Err(error("approval.in_use"));
            }
            if entry.state != ApprovalState::Granted {
                return Err(error("approval.already_used"));
            }
            entry.record.state = ApprovalRecordState::Invalidated;
            entry.record.secret.fill(0);
            entry.token.invalidate();
            entry.state = ApprovalState::Withdrawn;
            registry
                .states
                .insert(request.request_version_id.clone(), ApprovalState::Withdrawn);
            registry.sequence = registry.sequence.saturating_add(1);
            registry.sequence
        };
        self.0.publish(
            &request.request_version_id,
            ApprovalEventState::Revoked,
            Some(APPROVAL_USER_REVOKED),
            sequence,
        );
        Ok(ApprovalRevokeResponse {
            state: ApprovalState::Withdrawn,
        })
    }
}
