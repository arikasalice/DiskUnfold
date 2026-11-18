//! Test-only candidate validator; dynamic storage is not a production budget.
use super::{
    folder_attempt_contract::{
        AttemptEventPayload, ChildCorrelationCapability, DirectoryAttemptEvent,
        DirectoryAttemptKey, DirectoryChildFinal, SequencedAttemptEvent,
    },
    folder_attempt_history::{EndRecord, key_capacity},
    folder_attempt_validation::valid_child_correlations,
};
use crate::{BuildError, observation_store::ObservationStore};
use du_core_contracts::result::ResultItemKind;
use std::sync::atomic::AtomicU64;
use std::{collections::HashMap, mem::size_of, sync::atomic::Ordering};

/// Draft target only; the candidate's post-growth check does not enforce this limit.
pub(crate) const CANDIDATE_FOLDER_THRESHOLD: usize = 1 << 20;
/// Draft target only; the candidate's post-growth check does not enforce this limit.
pub(crate) const CANDIDATE_BUILD_THRESHOLD: usize = 32 << 20;
static NEXT_BUILD_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_build_id() -> Result<u64, BuildError> {
    NEXT_BUILD_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| BuildError::CapacityExceeded)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttemptStatus {
    Pending,
    Held,
    EndedUnproven,
}

#[derive(Clone, Debug)]
pub(super) struct AttemptState {
    pub(super) folder_node: u32,
    pub(super) events: Vec<SequencedAttemptEvent>,
    pub(super) end: Option<EndRecord>,
    pub(super) status: AttemptStatus,
}

impl AttemptState {
    fn new(folder_node: u32, first: SequencedAttemptEvent) -> Self {
        Self {
            folder_node,
            events: vec![first],
            end: None,
            status: AttemptStatus::Pending,
        }
    }

    fn hold(&mut self) {
        self.status = AttemptStatus::Held;
    }

    fn capacity(&self) -> usize {
        self.events
            .capacity()
            .saturating_mul(size_of::<SequencedAttemptEvent>())
            .saturating_add(
                self.events
                    .iter()
                    .map(|event| match &event.payload {
                        AttemptEventPayload::CallbackSeen { raw_name, .. } => raw_name.capacity(),
                        _ => 0,
                    })
                    .fold(0usize, usize::saturating_add),
            )
    }
}

#[derive(Debug, Default)]
pub(crate) struct AttemptLedger {
    build_id: u64,
    pub(super) attempts: HashMap<DirectoryAttemptKey, AttemptState>,
    pub(super) disabled: bool,
}

impl AttemptLedger {
    pub(crate) fn new(build_id: u64) -> Self {
        Self {
            build_id,
            ..Self::default()
        }
    }

    pub(crate) fn record(&mut self, event: DirectoryAttemptEvent, store: &ObservationStore) {
        if self.disabled {
            return;
        }
        match event {
            DirectoryAttemptEvent::Started {
                key,
                event_seq,
                correlation,
            } => self.start(key, event_seq, correlation, store),
            DirectoryAttemptEvent::CallbackSeen(seen) => {
                self.append(
                    seen.key,
                    seen.event_seq,
                    AttemptEventPayload::CallbackSeen {
                        callback_id: seen.callback_id,
                        child_id: seen.child_id,
                        raw_name: seen.raw_name,
                        decision: seen.decision,
                        phase: seen.phase,
                    },
                );
            }
            DirectoryAttemptEvent::ChildFinal {
                key,
                event_seq,
                child_id,
                classification,
            } => self.append(
                key,
                event_seq,
                AttemptEventPayload::ChildFinal {
                    child_id,
                    classification,
                },
            ),
            DirectoryAttemptEvent::Progress { key, event_seq } => {
                self.append(key, event_seq, AttemptEventPayload::Progress)
            }
            DirectoryAttemptEvent::PendingCleared {
                key,
                event_seq,
                child_id,
            } => self.append(
                key,
                event_seq,
                AttemptEventPayload::PendingCleared { child_id },
            ),
            DirectoryAttemptEvent::MetadataError { key, event_seq } => {
                self.append(key, event_seq, AttemptEventPayload::MetadataError)
            }
            DirectoryAttemptEvent::TransportFailure { key, event_seq } => {
                self.append(key, event_seq, AttemptEventPayload::TransportFailure)
            }
            DirectoryAttemptEvent::Cancelled { key, event_seq } => {
                self.append(key, event_seq, AttemptEventPayload::Cancelled)
            }
            DirectoryAttemptEvent::End {
                key,
                event_watermark_exclusive,
                callback_watermark_exclusive,
                child_count_exclusive,
            } => self.end(
                key,
                event_watermark_exclusive,
                callback_watermark_exclusive,
                child_count_exclusive,
            ),
        }
    }

    fn start(
        &mut self,
        key: DirectoryAttemptKey,
        event_seq: u64,
        correlation: Option<ChildCorrelationCapability>,
        store: &ObservationStore,
    ) {
        if key.build_id != self.build_id {
            self.disable();
            return;
        }
        if let Some(state) = self.attempts.get_mut(&key) {
            if event_seq == 0
                && state.events.first().is_some_and(|event| {
                    event.event_seq == 0
                        && event.payload == AttemptEventPayload::Started { correlation }
                })
            {
                return;
            }
            state.hold();
            return;
        }
        if event_seq != 0
            || self
                .attempts
                .keys()
                .any(|existing| existing.attempt_id == key.attempt_id)
        {
            self.disable();
            return;
        }
        let Some(node) = store.find(key.root_index, &key.raw_parent) else {
            self.disable();
            return;
        };
        if store.observation(node).map(|item| &item.kind) != Some(&ResultItemKind::Directory) {
            self.disable();
            return;
        }
        // Candidate-only path: reserve/accounting still happens after allocation.
        if self.attempts.try_reserve(1).is_err() {
            self.disable();
            return;
        }
        let first = SequencedAttemptEvent {
            event_seq,
            payload: AttemptEventPayload::Started { correlation },
        };
        self.attempts.insert(key, AttemptState::new(node, first));
        if self.capacity() > CANDIDATE_BUILD_THRESHOLD {
            self.disable();
        }
    }

    fn append(&mut self, key: DirectoryAttemptKey, event_seq: u64, payload: AttemptEventPayload) {
        let Some(state) = self.checked_state(&key) else {
            return;
        };
        let Ok(index) = usize::try_from(event_seq) else {
            state.hold();
            return;
        };
        if index < state.events.len() {
            if state.events[index].event_seq == event_seq && state.events[index].payload == payload
            {
                return;
            }
            state.hold();
            return;
        }
        if index != state.events.len() || state.status != AttemptStatus::Pending {
            state.hold();
            return;
        }
        let must_hold = matches!(
            &payload,
            AttemptEventPayload::PendingCleared { .. }
                | AttemptEventPayload::MetadataError
                | AttemptEventPayload::TransportFailure
                | AttemptEventPayload::Cancelled
                | AttemptEventPayload::ChildFinal {
                    classification: DirectoryChildFinal::Unresolved,
                    ..
                }
        );
        if state.events.try_reserve(1).is_err() {
            state.hold();
            return;
        }
        state
            .events
            .push(SequencedAttemptEvent { event_seq, payload });
        if must_hold {
            state.hold();
            return;
        }
        self.hold_after_threshold_exceeded(&key);
    }

    fn end(
        &mut self,
        key: DirectoryAttemptKey,
        event_watermark_exclusive: u64,
        callback_watermark_exclusive: u64,
        child_count_exclusive: u64,
    ) {
        let Some(state) = self.checked_state(&key) else {
            return;
        };
        let end = EndRecord {
            event_watermark_exclusive,
            callback_watermark_exclusive,
            child_count_exclusive,
        };
        if let Some(previous) = state.end {
            if previous == end {
                return;
            }
            state.hold();
            return;
        }
        let complete = state.status == AttemptStatus::Pending
            && u64::try_from(state.events.len()).ok() == Some(event_watermark_exclusive)
            && valid_child_correlations(
                &state.events,
                callback_watermark_exclusive,
                child_count_exclusive,
            );
        state.end = Some(end);
        state.status = if complete {
            AttemptStatus::EndedUnproven
        } else {
            AttemptStatus::Held
        };
        self.hold_after_threshold_exceeded(&key);
    }

    fn checked_state(&mut self, key: &DirectoryAttemptKey) -> Option<&mut AttemptState> {
        if key.build_id != self.build_id || !self.attempts.contains_key(key) {
            self.disable();
            return None;
        }
        self.attempts.get_mut(key)
    }

    // Diagnostic fail-closed response only: this runs after growth and is not a memory cap.
    fn hold_after_threshold_exceeded(&mut self, key: &DirectoryAttemptKey) {
        if self
            .attempts
            .get(key)
            .is_some_and(|state| state.capacity() > CANDIDATE_FOLDER_THRESHOLD)
        {
            if let Some(state) = self.attempts.get_mut(key) {
                state.hold();
            }
        }
        if self.capacity() > CANDIDATE_BUILD_THRESHOLD {
            self.disable();
        }
    }

    fn capacity(&self) -> usize {
        self.attempts
            .capacity()
            .saturating_mul(size_of::<(DirectoryAttemptKey, AttemptState)>())
            .saturating_add(
                self.attempts
                    .iter()
                    .map(|(key, state)| key_capacity(key).saturating_add(state.capacity()))
                    .fold(0usize, usize::saturating_add),
            )
    }

    fn disable(&mut self) {
        self.attempts = HashMap::new();
        self.disabled = true;
    }
}

#[cfg(test)]
#[path = "folder_attempts_tests.rs"]
mod tests;
