//! FR4.HD1.app-host.DD1 / FR10.HD1.app-host.DD1: approval state transitions and invalidation.

use super::*;

impl Runtime {
    pub(super) fn source(&self, id: &str) -> Option<DataSource> {
        let id = Id::parse(id).ok()?;
        self.versions
            .lock()
            .ok()?
            .read(&id)
            .ok()
            .map(|record| record.data_source)
    }

    pub(super) fn require_primary(&self) -> Result<(), EngineError> {
        self.primary_window
            .then_some(())
            .ok_or_else(|| error("approval.window_denied"))
    }

    pub(super) fn invalidate_ruleset_changes(&self, current: Option<u64>) {
        let Ok(_transition) = self.transition_lock.lock() else {
            return;
        };
        let changed = if let Ok(mut registry) = self.registry.lock() {
            let mut requests = Vec::new();
            for (request, entry) in &mut registry.entries {
                if entry.state == ApprovalState::Granted && entry.record.ruleset_version != current
                {
                    entry.record.state = ApprovalRecordState::Invalidated;
                    entry.record.secret.fill(0);
                    entry.token.invalidate();
                    entry.state = ApprovalState::Invalidated;
                    requests.push(request.clone());
                }
            }
            let mut changed = Vec::new();
            for request in requests {
                registry.sequence = registry.sequence.saturating_add(1);
                registry
                    .states
                    .insert(request.clone(), ApprovalState::Invalidated);
                changed.push((request, registry.sequence));
            }
            changed
        } else {
            Vec::new()
        };
        for (request, sequence) in changed {
            self.publish(
                &request,
                ApprovalEventState::Revoked,
                Some(APPROVAL_RULESET_CHANGED),
                sequence,
            );
        }
    }

    pub(super) fn observe_event(&self, event: EventEnvelope) {
        match event.topic.as_str() {
            "job.finished" => self.observe_change_finished(&event),
            "plan.version_fixed" => {
                if let Some(id) = event
                    .body
                    .get("replacedPlanVersionId")
                    .and_then(serde_json::Value::as_str)
                    && let Ok(id) = Id::parse(id)
                {
                    self.invalidate_one(&id, APPROVAL_PLAN_SUPERSEDED);
                }
            }
            "protect.ruleset_changed" => self.invalidate_all(APPROVAL_RULESET_CHANGED),
            "restore.info_cleared" => self.invalidate_event_request(
                &event.body,
                du_core_contracts::approval_ipc::APPROVAL_RESTORE_INFO_CLEARED,
            ),
            "move_undo.info_cleared" => self.invalidate_event_request(
                &event.body,
                du_core_contracts::approval_ipc::APPROVAL_MOVE_UNDO_INFO_CLEARED,
            ),
            _ => {}
        }
    }

    pub(super) fn invalidate_event_request(&self, body: &serde_json::Value, reason: &'static str) {
        if let Some(id) = body
            .get("requestVersionId")
            .and_then(serde_json::Value::as_str)
            && let Ok(id) = Id::parse(id)
        {
            self.invalidate_one(&id, reason);
        }
    }

    pub(super) fn invalidate_one(&self, request: &Id, reason: &'static str) {
        let Ok(_transition) = self.transition_lock.lock() else {
            return;
        };
        let sequence = if let Ok(mut registry) = self.registry.lock() {
            if let Some(entry) = registry.entries.get_mut(request)
                && entry.state == ApprovalState::Granted
            {
                entry.record.state = ApprovalRecordState::Invalidated;
                entry.record.secret.fill(0);
                entry.token.invalidate();
                entry.state = ApprovalState::Invalidated;
                registry
                    .states
                    .insert(request.clone(), ApprovalState::Invalidated);
                registry.sequence = registry.sequence.saturating_add(1);
                Some(registry.sequence)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(sequence) = sequence {
            self.publish(request, ApprovalEventState::Revoked, Some(reason), sequence);
        }
    }

    pub(super) fn invalidate_all(&self, reason: &'static str) {
        let Ok(_transition) = self.transition_lock.lock() else {
            return;
        };
        let changed = if let Ok(mut registry) = self.registry.lock() {
            let mut requests = Vec::new();
            for (id, entry) in &mut registry.entries {
                if entry.state == ApprovalState::Granted {
                    entry.record.state = ApprovalRecordState::Invalidated;
                    entry.record.secret.fill(0);
                    entry.token.invalidate();
                    entry.state = ApprovalState::Invalidated;
                    requests.push(id.clone());
                }
            }
            let mut changed = Vec::new();
            for id in requests {
                registry.sequence = registry.sequence.saturating_add(1);
                registry
                    .states
                    .insert(id.clone(), ApprovalState::Invalidated);
                changed.push((id, registry.sequence));
            }
            changed
        } else {
            Vec::new()
        };
        for (request, sequence) in changed {
            self.publish(
                &request,
                ApprovalEventState::Revoked,
                Some(reason),
                sequence,
            );
        }
    }

    pub(super) fn retire(&self) {
        let Ok(_transition) = self.transition_lock.lock() else {
            return;
        };
        if let Ok(mut registry) = self.registry.lock() {
            registry.entries.clear();
            registry.states.clear();
            registry.session_id = None;
        }
        if let Some(change) = &self.change {
            change
                .executions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
            change
                .cursor_key
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .fill(0);
            let _ = change.undo.clear();
        }
    }

    pub(super) fn mark_approval_spent(&self, request: &Id) {
        if let Ok(mut registry) = self.registry.lock()
            && let Some(entry) = registry.entries.get_mut(request)
            && entry.record.state == ApprovalRecordState::Consumed
        {
            entry.state = ApprovalState::Spent;
            registry
                .states
                .insert(request.clone(), ApprovalState::Spent);
        }
    }

    fn observe_change_finished(&self, event: &EventEnvelope) {
        let Some(change) = &self.change else {
            return;
        };
        let (Some(job_ref), Some(end_state)) = (
            event.job_ref.as_ref(),
            event.body.get("state").and_then(serde_json::Value::as_str),
        ) else {
            return;
        };
        let audit = {
            let mut executions = change
                .executions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(execution) = executions.get_mut(job_ref) else {
                return;
            };
            if execution.audit_finished {
                return;
            }
            execution.audit_finished = true;
            Some((
                execution.request_version_id.clone(),
                execution.audit_facts.clone(),
            ))
        };
        let Some((request_id, facts)) = audit else {
            return;
        };
        let mut finish_facts = facts;
        if let Some(check_id) = super::new_id(self.random.as_ref()) {
            finish_facts.check_id = check_id;
            if let Ok(mut recorder) = change.recorder.lock() {
                let _ = du_change_executor::record_check_finish(
                    &mut *recorder,
                    &finish_facts,
                    end_state,
                );
            }
        }
        let sequence = {
            let Ok(_transition) = self.transition_lock.lock() else {
                return;
            };
            let Ok(mut registry) = self.registry.lock() else {
                return;
            };
            let Some(entry) = registry.entries.get_mut(&request_id) else {
                return;
            };
            if entry.record.state != ApprovalRecordState::Consumed
                || entry.state != ApprovalState::InUse
            {
                return;
            }
            entry.state = ApprovalState::Spent;
            registry
                .states
                .insert(request_id.clone(), ApprovalState::Spent);
            registry.sequence = registry.sequence.saturating_add(1);
            registry.sequence
        };
        self.publish(&request_id, ApprovalEventState::Consumed, None, sequence);
    }

    pub(super) fn publish(
        &self,
        request: &Id,
        state: ApprovalEventState,
        reason: Option<&str>,
        sequence: u64,
    ) {
        let Some(occurred_at) = Timestamp::from_unix_millis(now_ms()) else {
            return;
        };
        let body = ApprovalStateChanged {
            request_version_id: request.clone(),
            state,
            reason_key: reason.map(str::to_owned),
            sequence,
            occurred_at: occurred_at.clone(),
        };
        let Ok(body) = serde_json::to_value(body) else {
            return;
        };
        self.events.publish_host_event(EventEnvelope {
            topic: APPROVAL_STATE_CHANGED.into(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id: None,
            job_ref: None,
            result_version: None,
            occurred_at,
            body,
        });
    }
}
