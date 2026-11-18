//! Production BuildHandle receiver for draft correlated directory-attempt events.
use crate::observation_store::ObservationStore;
use crate::{
    folder_evidence_api::{
        FolderAttemptError, FolderAttemptFinal, FolderAttemptInput, FolderAttemptStatus,
        FolderAttemptToken,
    },
    folder_evidence_budget::{
        FolderEvidenceBudget, FolderEvidenceLease, FolderScopeHandle, MAX_FOLDER_MANAGED_BYTES,
    },
};
use du_core_contracts::scan_observation::CorrelatedDirectoryEvent;
use std::{mem::size_of, sync::Arc};

const MAX_ATTEMPTS_PER_BUILD: usize = 4096;
const INITIAL_ATTEMPT_REGISTRY_CHUNK: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndRecord {
    pub(crate) event_watermark_exclusive: u64,
    pub(crate) child_count_exclusive: u64,
    pub(crate) outcome: Result<(), FolderAttemptError>,
}

pub(crate) struct StoredAttemptEvent {
    pub(crate) event_seq: u64,
    pub(crate) input: Option<FolderAttemptInput>,
    _payload_lease: Option<FolderEvidenceLease>,
}

pub(crate) struct AttemptState {
    pub(crate) token: FolderAttemptToken,
    pub(crate) status: FolderAttemptStatus,
    pub(crate) events: Vec<StoredAttemptEvent>,
    pub(crate) end: Option<EndRecord>,
    pub(crate) event_capacity_lease: Option<FolderEvidenceLease>,
    pub(crate) payload_bytes: usize,
    pub(crate) scope: FolderScopeHandle,
}

pub(crate) struct FolderAttemptLedger {
    generation: u64,
    pub(crate) budget: Arc<FolderEvidenceBudget>,
    pub(crate) attempts: Vec<AttemptState>,
    registry_lease: Option<FolderEvidenceLease>,
    admission_disabled: bool,
}

impl FolderAttemptLedger {
    pub(crate) fn new(generation: u64, budget: Arc<FolderEvidenceBudget>) -> Self {
        Self {
            generation,
            budget,
            attempts: Vec::new(),
            registry_lease: None,
            admission_disabled: false,
        }
    }

    pub(crate) fn disable_admission(&mut self) {
        self.admission_disabled = true;
    }

    pub(crate) fn hold_pending(&mut self) {
        for state in &mut self.attempts {
            if state.status == FolderAttemptStatus::Pending {
                state.status = FolderAttemptStatus::Held;
            }
        }
    }

    pub(crate) fn discard(&mut self) {
        self.attempts = Vec::new();
        self.registry_lease = None;
        self.admission_disabled = true;
    }

    pub(crate) fn begin(
        &mut self,
        root_index: u32,
        folder_node: u32,
        attempt_id: u64,
        root_encoding: u8,
        root_path: &str,
        raw_parent: &[Vec<u8>],
    ) -> Result<FolderAttemptToken, FolderAttemptError> {
        if self.admission_disabled {
            return Err(FolderAttemptError::CapacityExceeded);
        }
        if let Some(existing) = self
            .attempts
            .iter_mut()
            .find(|state| state.token.attempt_id == attempt_id)
        {
            existing.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::AttemptIdReused);
        }
        if self.attempts.len() >= MAX_ATTEMPTS_PER_BUILD {
            self.admission_disabled = true;
            return Err(FolderAttemptError::CapacityExceeded);
        }
        let Some(scope) =
            self.budget
                .folder_memory(root_index, root_encoding, root_path, raw_parent)
        else {
            self.admission_disabled = true;
            return Err(FolderAttemptError::CapacityExceeded);
        };
        let token = FolderAttemptToken {
            build_generation: self.generation,
            attempt_id,
            root_index,
            folder_node,
        };
        let mut events = Vec::new();
        let event_capacity = 4usize;
        let Some(event_bytes) = size_of::<StoredAttemptEvent>().checked_mul(event_capacity) else {
            self.admission_disabled = true;
            return Err(FolderAttemptError::CapacityExceeded);
        };
        let Some(mut event_lease) = self.budget.reserve_for_folder(&scope, event_bytes) else {
            self.admission_disabled = true;
            return Err(FolderAttemptError::CapacityExceeded);
        };
        if events.try_reserve_exact(event_capacity).is_err() {
            self.admission_disabled = true;
            return Err(FolderAttemptError::CapacityExceeded);
        }
        if !event_lease.reconcile_capacity(
            &self.budget,
            Some(&scope),
            size_of::<StoredAttemptEvent>(),
            events.capacity(),
        ) {
            self.admission_disabled = true;
            return Err(FolderAttemptError::CapacityExceeded);
        }
        let required = self.attempts.len().saturating_add(1);
        if required > self.attempts.capacity() {
            let Some(target_capacity) =
                registry_growth_capacity(self.attempts.capacity(), required)
            else {
                self.admission_disabled = true;
                return Err(FolderAttemptError::CapacityExceeded);
            };
            // Build a separately reserved buffer so a failed actual-capacity charge leaves
            // the previous registry and its entries intact.
            let Some(lease) =
                try_grow_attempt_slots(&mut self.attempts, target_capacity, &self.budget)
            else {
                self.admission_disabled = true;
                return Err(FolderAttemptError::CapacityExceeded);
            };
            let previous_lease = self.registry_lease.replace(lease);
            drop(previous_lease);
        }
        self.attempts.push(AttemptState {
            token,
            status: FolderAttemptStatus::Pending,
            events,
            end: None,
            event_capacity_lease: Some(event_lease),
            payload_bytes: 0,
            scope,
        });
        Ok(token)
    }

    pub(crate) fn record(
        &mut self,
        token: FolderAttemptToken,
        event_seq: u64,
        input: FolderAttemptInput,
    ) -> Result<(), FolderAttemptError> {
        let Some(index) = self.index_for(token)? else {
            return Err(FolderAttemptError::UnknownAttempt);
        };
        let budget = Arc::clone(&self.budget);
        let state = &mut self.attempts[index];
        let Ok(seq_index) = usize::try_from(event_seq) else {
            state.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::SequenceGap);
        };
        if seq_index < state.events.len() {
            let prior = &state.events[seq_index];
            if prior.event_seq == event_seq && prior.input.as_ref() == Some(&input) {
                return if matches!(
                    state.status,
                    FolderAttemptStatus::Pending | FolderAttemptStatus::EndedUnproven
                ) {
                    Ok(())
                } else {
                    Err(FolderAttemptError::Conflict)
                };
            }
            state.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::Conflict);
        }
        if seq_index != state.events.len() {
            state.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::SequenceGap);
        }
        if state.status != FolderAttemptStatus::Pending {
            state.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::Conflict);
        }
        if matches!(
            &input,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::End { .. })
        ) {
            state.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::InvalidEnd);
        }

        let Some(payload_bytes) = input.managed_payload_capacity() else {
            state.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::CapacityExceeded);
        };
        let Some(required) = state.events.len().checked_add(1) else {
            state.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::CapacityExceeded);
        };
        let grows = required > state.events.capacity();
        let target_capacity = if grows {
            state.events.capacity().saturating_mul(2).max(required)
        } else {
            state.events.capacity()
        };
        let Some(target_event_bytes) = target_capacity.checked_mul(size_of::<StoredAttemptEvent>())
        else {
            state.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::CapacityExceeded);
        };
        if target_event_bytes
            .checked_add(state.payload_bytes)
            .and_then(|bytes| bytes.checked_add(payload_bytes))
            .is_none_or(|bytes| bytes > MAX_FOLDER_MANAGED_BYTES)
        {
            state.status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::CapacityExceeded);
        }

        let payload_lease = if payload_bytes == 0 {
            None
        } else {
            match budget.reserve_for_folder(&state.scope, payload_bytes) {
                Some(lease) => Some(lease),
                None => {
                    state.status = FolderAttemptStatus::Held;
                    return Err(FolderAttemptError::CapacityExceeded);
                }
            }
        };
        let new_event_lease = if grows {
            match try_grow_event_buffer(&mut state.events, target_capacity, &budget, &state.scope) {
                Some(lease) => Some(lease),
                None => {
                    state.status = FolderAttemptStatus::Held;
                    return Err(FolderAttemptError::CapacityExceeded);
                }
            }
        } else {
            None
        };

        if grows {
            let Some(lease) = new_event_lease else {
                state.status = FolderAttemptStatus::Held;
                return Err(FolderAttemptError::CapacityExceeded);
            };
            let previous_lease = state.event_capacity_lease.replace(lease);
            drop(previous_lease);
        }
        state.payload_bytes += payload_bytes;
        state.events.push(StoredAttemptEvent {
            event_seq,
            input: Some(input),
            _payload_lease: payload_lease,
        });
        if matches!(
            state.events.last().and_then(|event| event.input.as_ref()),
            Some(
                FolderAttemptInput::PendingCleared { .. }
                    | FolderAttemptInput::TransportFailure
                    | FolderAttemptInput::Cancelled
                    | FolderAttemptInput::Final {
                        outcome: FolderAttemptFinal::Unresolved,
                        ..
                    }
            )
        ) {
            state.status = FolderAttemptStatus::Held;
        }
        Ok(())
    }

    pub(crate) fn end(
        &mut self,
        token: FolderAttemptToken,
        event_watermark_exclusive: u64,
        child_count_exclusive: u64,
        store: &ObservationStore,
    ) -> Result<(), FolderAttemptError> {
        let Some(index) = self.index_for(token)? else {
            return Err(FolderAttemptError::UnknownAttempt);
        };
        if let Some(previous) = self.attempts[index].end {
            if previous.event_watermark_exclusive == event_watermark_exclusive
                && previous.child_count_exclusive == child_count_exclusive
            {
                if let Err(error) = previous.outcome {
                    return Err(error);
                }
                return if self.attempts[index].status == FolderAttemptStatus::EndedUnproven {
                    Ok(())
                } else {
                    Err(FolderAttemptError::InvalidEnd)
                };
            }
            self.attempts[index].status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::Conflict);
        }
        let barrier_matches = self.attempts[index].status == FolderAttemptStatus::Pending
            && usize::try_from(event_watermark_exclusive).ok()
                == Some(self.attempts[index].events.len());
        let validation = if barrier_matches {
            Some(super::folder_evidence_validation::valid_attempt(
                &self.attempts[index],
                child_count_exclusive,
                &self.attempts,
                store,
                &self.budget,
            ))
        } else {
            None
        };
        let outcome = match validation {
            Some(Ok(true)) => Ok(()),
            Some(Err(error)) => Err(error),
            Some(Ok(false)) | None => Err(FolderAttemptError::InvalidEnd),
        };
        self.attempts[index].end = Some(EndRecord {
            event_watermark_exclusive,
            child_count_exclusive,
            outcome,
        });
        self.attempts[index].status = if outcome.is_ok() {
            FolderAttemptStatus::EndedUnproven
        } else {
            FolderAttemptStatus::Held
        };
        outcome
    }

    pub(crate) fn hold_all(&mut self) {
        for state in &mut self.attempts {
            state.status = FolderAttemptStatus::Held;
        }
    }

    fn index_for(
        &mut self,
        token: FolderAttemptToken,
    ) -> Result<Option<usize>, FolderAttemptError> {
        if token.build_generation != self.generation {
            return Err(FolderAttemptError::WrongBuild);
        }
        let Some(index) = self
            .attempts
            .iter()
            .position(|state| state.token.attempt_id == token.attempt_id)
        else {
            return Ok(None);
        };
        if self.attempts[index].token != token {
            self.attempts[index].status = FolderAttemptStatus::Held;
            return Err(FolderAttemptError::Conflict);
        }
        Ok(Some(index))
    }
}

fn registry_growth_capacity(current: usize, required: usize) -> Option<usize> {
    if required > MAX_ATTEMPTS_PER_BUILD {
        return None;
    }
    if required <= current {
        return Some(current);
    }
    let target = if current == 0 {
        INITIAL_ATTEMPT_REGISTRY_CHUNK
    } else {
        current
            .saturating_mul(2)
            .max(required)
            .min(MAX_ATTEMPTS_PER_BUILD)
    };
    (target >= required).then_some(target)
}

/// Uses the same fallible reserve operation as production so capacity-overflow tests do not
/// require exhausting the process allocator.
fn try_grow_attempt_slots(
    attempts: &mut Vec<AttemptState>,
    target: usize,
    budget: &Arc<FolderEvidenceBudget>,
) -> Option<FolderEvidenceLease> {
    let requested_bytes = target.checked_mul(size_of::<AttemptState>())?;
    let mut lease = budget.reserve(requested_bytes)?;
    let mut replacement = Vec::new();
    replacement.try_reserve_exact(target).ok()?;
    if replacement.capacity() > MAX_ATTEMPTS_PER_BUILD
        || !lease.reconcile_capacity(
            budget,
            None,
            size_of::<AttemptState>(),
            replacement.capacity(),
        )
    {
        return None;
    }
    replacement.append(attempts);
    let previous = std::mem::replace(attempts, replacement);
    drop(previous);
    Some(lease)
}

fn try_grow_event_buffer(
    events: &mut Vec<StoredAttemptEvent>,
    target: usize,
    budget: &Arc<FolderEvidenceBudget>,
    scope: &FolderScopeHandle,
) -> Option<FolderEvidenceLease> {
    let requested_bytes = target.checked_mul(size_of::<StoredAttemptEvent>())?;
    let mut lease = budget.reserve_for_folder(scope, requested_bytes)?;
    let mut replacement = Vec::new();
    replacement.try_reserve_exact(target).ok()?;
    if !lease.reconcile_capacity(
        budget,
        Some(scope),
        size_of::<StoredAttemptEvent>(),
        replacement.capacity(),
    ) {
        return None;
    }
    replacement.append(events);
    let previous = std::mem::replace(events, replacement);
    drop(previous);
    Some(lease)
}

#[cfg(test)]
#[path = "folder_evidence_receiver_tests.rs"]
mod tests;
