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
