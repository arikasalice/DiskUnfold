//! Test-only immutable snapshots with reservation before any deep copy.
use super::{
    folder_attempt_contract::{AttemptEventPayload, DirectoryAttemptKey, SequencedAttemptEvent},
    folder_attempts::{AttemptLedger, AttemptStatus},
};
use std::{
    collections::HashMap,
    mem::size_of,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(crate) const MAX_HISTORY_ACCOUNTED_BYTES: usize = 32 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndRecord {
    pub(crate) event_watermark_exclusive: u64,
    pub(crate) callback_watermark_exclusive: u64,
    pub(crate) child_count_exclusive: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct PinnedAttempt {
    pub(crate) key: DirectoryAttemptKey,
    pub(crate) folder_item: u32,
    pub(crate) status: AttemptStatus,
    pub(crate) events: Vec<SequencedAttemptEvent>,
    pub(crate) end: Option<EndRecord>,
}

/// Immutable copy of received evidence for this publication boundary only.
/// It does not claim observations share one instant or reveal undetected changes.
#[derive(Debug)]
pub(crate) struct FolderEvidenceSnapshot {
    // Field order keeps the reservation until all copied buffers are released.
    pub(crate) attempts: Vec<PinnedAttempt>,
    _lease: HistoryLease,
}

#[derive(Debug, Default)]
pub(crate) struct HistoryBudget {
    used: AtomicUsize,
}

#[derive(Debug)]
pub(crate) struct HistoryLease {
    budget: Arc<HistoryBudget>,
    bytes: usize,
}

impl HistoryBudget {
    pub(crate) fn used_bytes(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }

    pub(crate) fn reserve(self: &Arc<Self>, bytes: usize) -> Option<HistoryLease> {
        let mut current = self.used.load(Ordering::Relaxed);
        loop {
            let next = current.checked_add(bytes)?;
            if next > MAX_HISTORY_ACCOUNTED_BYTES {
                return None;
            }
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(HistoryLease {
                        budget: Arc::clone(self),
                        bytes,
                    });
                }
                Err(actual) => current = actual,
            }
        }
    }
}

impl Drop for HistoryLease {
    fn drop(&mut self) {
        if self.bytes != 0 {
            self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
        }
    }
}

impl AttemptLedger {
    pub(crate) fn snapshot(
        &self,
        built: &crate::tree::BuiltItems,
        history: &Arc<HistoryBudget>,
    ) -> Option<FolderEvidenceSnapshot> {
        if self.disabled {
            return None;
        }
        let reservation = snapshot_reservation(&self.attempts)?;
        let lease = history.reserve(reservation)?;
        let mut attempts = Vec::new();
        attempts.try_reserve_exact(self.attempts.len()).ok()?;
        for (key, state) in &self.attempts {
            let folder_item = *built.item_of_node.get(state.folder_node as usize)?;
            let key = try_clone_key(key)?;
            let mut events = Vec::new();
            events.try_reserve_exact(state.events.len()).ok()?;
            for event in &state.events {
                events.push(try_clone_event(event)?);
            }
            attempts.push(PinnedAttempt {
                key,
                folder_item,
                status: state.status,
                events,
                end: state.end,
            });
        }
        Some(FolderEvidenceSnapshot {
            attempts,
            _lease: lease,
        })
    }
}

fn snapshot_reservation(
    attempts: &HashMap<DirectoryAttemptKey, super::folder_attempts::AttemptState>,
) -> Option<usize> {
    // Counts requested element slots and payload lengths; allocator metadata and spare capacity are excluded.
    let mut bytes = attempts.len().checked_mul(size_of::<PinnedAttempt>())?;
    for (key, state) in attempts {
        bytes = bytes.checked_add(key.raw_parent.len().checked_mul(size_of::<Vec<u8>>())?)?;
        for component in &key.raw_parent {
            bytes = bytes.checked_add(component.len())?;
        }
        bytes = bytes.checked_add(
            state
                .events
                .len()
                .checked_mul(size_of::<SequencedAttemptEvent>())?,
        )?;
        for event in &state.events {
            if let AttemptEventPayload::CallbackSeen { raw_name, .. } = &event.payload {
                bytes = bytes.checked_add(raw_name.len())?;
            }
        }
    }
    Some(bytes)
}

pub(super) fn key_capacity(key: &DirectoryAttemptKey) -> usize {
    key.raw_parent
        .capacity()
        .saturating_mul(size_of::<Vec<u8>>())
        .saturating_add(
            key.raw_parent
                .iter()
                .map(Vec::capacity)
                .fold(0usize, usize::saturating_add),
        )
}

fn try_clone_key(key: &DirectoryAttemptKey) -> Option<DirectoryAttemptKey> {
    let mut raw_parent = Vec::new();
    raw_parent.try_reserve_exact(key.raw_parent.len()).ok()?;
    for component in &key.raw_parent {
        raw_parent.push(try_clone_bytes(component)?);
    }
    Some(DirectoryAttemptKey {
        build_id: key.build_id,
        root_index: key.root_index,
        raw_parent,
        attempt_id: key.attempt_id,
    })
}

fn try_clone_event(event: &SequencedAttemptEvent) -> Option<SequencedAttemptEvent> {
    let payload = match &event.payload {
        AttemptEventPayload::Started { correlation } => AttemptEventPayload::Started {
            correlation: *correlation,
        },
        AttemptEventPayload::CallbackSeen {
            callback_id,
            child_id,
            raw_name,
            decision,
            phase,
        } => AttemptEventPayload::CallbackSeen {
            callback_id: *callback_id,
            child_id: *child_id,
            raw_name: try_clone_bytes(raw_name)?,
            decision: *decision,
            phase: *phase,
        },
        AttemptEventPayload::ChildFinal {
            child_id,
            classification,
        } => AttemptEventPayload::ChildFinal {
            child_id: *child_id,
            classification: classification.clone(),
        },
        AttemptEventPayload::Progress => AttemptEventPayload::Progress,
        AttemptEventPayload::PendingCleared { child_id } => AttemptEventPayload::PendingCleared {
            child_id: *child_id,
        },
        AttemptEventPayload::MetadataError => AttemptEventPayload::MetadataError,
        AttemptEventPayload::TransportFailure => AttemptEventPayload::TransportFailure,
        AttemptEventPayload::Cancelled => AttemptEventPayload::Cancelled,
    };
    Some(SequencedAttemptEvent {
        event_seq: event.event_seq,
        payload,
    })
}

fn try_clone_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len()).ok()?;
    copy.extend_from_slice(bytes);
    Some(copy)
}
