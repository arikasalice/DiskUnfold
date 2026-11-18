//! Per-frame staging for an optional correlated enumeration.
//!
//! This boundary is not enabled until the result receiver owns its events.
use super::{Directory, QueuedEntry};
use crate::scan::parallel::correlated_budget::{
    self, CorrelatedBudget, MAX_EVENT_PAYLOAD_BYTES, Permit,
};
use du_core_contracts::{
    Timestamp,
    ports::{PortError, PortErrorKind},
    scan_observation::{CorrelatedDirectoryEvent, FilterDecision},
};
use std::{mem::size_of, sync::Arc};

/// Bounded unmatched callbacks; allocator overhead is additional.
const MAX_PENDING_ADMITS: usize = 256;
const MAX_PENDING_NAME_BYTES: usize = 64 * 1024;

struct PendingAdmit {
    child_id: u64,
    start: usize,
    len: usize,
}

#[derive(Default)]
enum State {
    #[default]
    Unselected,
    Legacy,
    Active,
    Finished,
    Failed,
}

/// This state moves with its frame, including when another worker takes it.
#[derive(Default)]
pub(crate) struct Attempt {
    state: State,
    next_seq: u64,
    token: Option<du_analysis_index::FolderAttemptToken>,
    pending_admits: Vec<PendingAdmit>,
    pending_names: Vec<u8>,
    pending_name_bytes: usize,
    budget: Option<Arc<CorrelatedBudget>>,
    attempt_permit: Option<Permit>,
    event_permit: Option<Permit>,
}

// Inline values stay under the attempt permit and avoid an allocation per callback.
#[allow(clippy::large_enum_variant)]
#[allow(dead_code)]
pub(in crate::scan::parallel) enum Step {
    Legacy,
    Progress,
    Event {
        seq: u64,
        payload: CorrelatedDirectoryEvent,
        observation: Option<QueuedEntry>,
    },
    /// An adapter event was acknowledged; any observation remains owned by the F02 frame.
    Accepted {
        observation: Option<QueuedEntry>,
    },
    End {
        event_watermark_exclusive: u64,
        child_count_exclusive: u64,
    },
    /// The attempt failed, but the caller still owns the returned adapter event.
    Returned(CorrelatedDirectoryEvent),
    Failed,
}

#[allow(dead_code)]
impl Attempt {
    /// Reserves a bounded frame slot and all callback storage before selecting the adapter path.
    pub(in crate::scan::parallel) fn prepare(&mut self, budget: &Arc<CorrelatedBudget>) -> bool {
        if self.attempt_permit.is_some() {
            return true;
        }
        let callback_slots = match MAX_PENDING_ADMITS.checked_mul(size_of::<PendingAdmit>()) {
            Some(bytes) => bytes,
            None => return false,
        };
        let Some(base_bytes) = size_of::<Self>()
            .checked_add(callback_slots)
            .and_then(|bytes| bytes.checked_add(MAX_PENDING_NAME_BYTES))
        else {
            return false;
        };
        let Some(mut permit) = budget.reserve_attempt(base_bytes) else {
            return false;
        };
        let mut pending_admits = Vec::new();
        if pending_admits
            .try_reserve_exact(MAX_PENDING_ADMITS)
            .is_err()
            || pending_admits.capacity() > MAX_PENDING_ADMITS
        {
            return false;
        }
        let mut pending_names = Vec::new();
        if pending_names
            .try_reserve_exact(MAX_PENDING_NAME_BYTES)
            .is_err()
        {
            return false;
        }
        if pending_names.capacity() > MAX_PENDING_NAME_BYTES {
            let excess = pending_names.capacity() - MAX_PENDING_NAME_BYTES;
            if !permit.add(excess) {
                return false;
            }
        }
        pending_names.resize(MAX_PENDING_NAME_BYTES, 0);
        let actual = size_of::<Self>()
            .checked_add(
                pending_admits
                    .capacity()
                    .saturating_mul(size_of::<PendingAdmit>()),
            )
            .and_then(|bytes| bytes.checked_add(pending_names.capacity()));
        let Some(actual) = actual else {
            return false;
        };
        if !permit.resize(actual) {
            return false;
        }
        self.pending_admits = pending_admits;
        self.pending_names = pending_names;
        self.pending_name_bytes = 0;
        self.budget = Some(Arc::clone(budget));
        self.attempt_permit = Some(permit);
        true
    }

    /// Selection does not consume a directory entry. Only Unsupported permits fallback.
    pub(in crate::scan::parallel) fn select(
        &mut self,
        directory: &Directory,
        budget: &Arc<CorrelatedBudget>,
    ) -> Step {
        if !matches!(self.state, State::Unselected) {
            self.state = State::Failed;
            return Step::Failed;
        }
        if !self.prepare(budget) {
            self.state = State::Legacy;
            return Step::Legacy;
        }
        match directory.with(|dir| dir.begin_correlated_filtered()) {
            Some(Ok(())) => {
                self.state = State::Active;
                // Registration is outside the event sequence.
                self.next_seq = 0;
                Step::Progress
            }
            Some(Err(PortError {
                kind: PortErrorKind::Unsupported,
                ..
            })) => {
                self.state = State::Legacy;
                self.release_buffers();
                Step::Legacy
            }
            _ => {
                self.state = State::Failed;
                self.release_buffers();
                Step::Failed
            }
        }
    }

    /// A full or disconnected channel fails the attempt instead of losing evidence.
    /// The adapter call returns before the bounded send is attempted.
    pub(in crate::scan::parallel) fn next(
        &mut self,
        directory: &Directory,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
        cancelled: &dyn Fn() -> bool,
    ) -> Step {
        if !matches!(self.state, State::Active) || cancelled() {
            self.state = State::Failed;
            return Step::Failed;
        }
        let Some(budget) = self.budget.as_ref().cloned() else {
            self.state = State::Failed;
            return Step::Failed;
        };
        let Some(event_bytes) = MAX_EVENT_RESERVATION else {
            self.state = State::Failed;
            return Step::Failed;
        };
        let Some(permit) = budget.reserve(event_bytes) else {
            // The adapter is already in correlated mode. Stop producing optional
            // evidence, then keep ordinary F02 observations flowing without a clone.
            self.evidence_failed();
            return self.next_without_evidence(directory, include, clock, cancelled);
        };
        self.event_permit = Some(permit);
        let Some(Ok(event)) = directory.with(|dir| dir.next_correlated_event(include, clock))
        else {
            self.state = State::Failed;
            self.event_permit = None;
            return Step::Failed;
        };
        // A cancel during the OS call invalidates the whole attempt, including EOF.
        if cancelled() {
            self.state = State::Failed;
            return match event {
                CorrelatedDirectoryEvent::End { .. } | CorrelatedDirectoryEvent::Progress => {
                    self.event_permit = None;
                    Step::Failed
                }
                event @ CorrelatedDirectoryEvent::Observation { .. } => Step::Returned(event),
                event
                    if correlated_budget::event_payload_capacity(&event)
                        .is_some_and(|bytes| bytes <= MAX_EVENT_PAYLOAD_BYTES) =>
                {
                    Step::Returned(event)
                }
                _ => {
                    self.event_permit = None;
                    Step::Failed
                }
            };
        }
        match event {
            CorrelatedDirectoryEvent::Progress => {
                self.event_permit = None;
                Step::Progress
            }
            CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            } => {
                if !self.pending_admits.is_empty() {
                    self.state = State::Failed;
                    self.event_permit = None;
                    return Step::Failed;
                }
                self.state = State::Finished;
                self.event_permit = None;
                Step::End {
                    event_watermark_exclusive: self.next_seq,
                    child_count_exclusive,
                }
            }
            payload => {
                if correlated_budget::event_payload_capacity(&payload)
                    .is_none_or(|bytes| bytes > MAX_EVENT_PAYLOAD_BYTES)
                {
                    self.state = State::Failed;
                    self.event_permit = None;
                    return match payload {
                        observation @ CorrelatedDirectoryEvent::Observation { .. } => {
                            Step::Returned(observation)
                        }
                        _ => Step::Failed,
                    };
                }
                let Some(next) = self.next_seq.checked_add(1) else {
                    self.state = State::Failed;
                    self.event_permit = None;
                    return Step::Failed;
                };
                // Keep the ID with the observation until worker admission is finished.
                let observation = match &payload {
                    CorrelatedDirectoryEvent::Observation { child_id, entry } => {
                        let Some(copy) = correlated_budget::try_clone_timed(entry) else {
                            self.state = State::Failed;
                            return Step::Returned(payload);
                        };
                        Some(QueuedEntry {
                            timed: copy,
                            child_id: Some(*child_id),
                            child_attempt: None,
                            payload_permit: None,
                        })
                    }
                    _ => None,
                };
                let callback = match &payload {
                    CorrelatedDirectoryEvent::FilterCallback {
                        child_id,
                        raw_name,
                        decision: FilterDecision::Admit,
                        ..
                    } => Some((*child_id, raw_name)),
                    _ => None,
                };
                if let Some((child_id, raw_name)) = callback {
                    if self
                        .pending_admits
                        .iter()
                        .any(|pending| pending.child_id == child_id)
                        || self.pending_admits.len() == MAX_PENDING_ADMITS
                        || self.pending_name_bytes.saturating_add(raw_name.len())
                            > MAX_PENDING_NAME_BYTES
                    {
                        self.state = State::Failed;
                        self.event_permit = None;
                        return Step::Failed;
                    }
                    let start = self.pending_name_bytes;
                    let Some(end) = start.checked_add(raw_name.len()) else {
                        self.state = State::Failed;
                        self.event_permit = None;
                        return Step::Failed;
                    };
                    self.pending_names[start..end].copy_from_slice(raw_name);
                    self.pending_name_bytes = end;
                    self.pending_admits.push(PendingAdmit {
                        child_id,
                        start,
                        len: raw_name.len(),
                    });
                }
                self.next_seq = next;
                Step::Event {
                    seq: self.next_seq - 1,
                    payload,
                    observation,
                }
            }
        }
    }

    fn next_without_evidence(
        &mut self,
        directory: &Directory,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
        cancelled: &dyn Fn() -> bool,
    ) -> Step {
        if !matches!(self.state, State::Active) || cancelled() {
            self.state = State::Failed;
            return Step::Failed;
        }
        let Some(Ok(event)) = directory.with(|dir| dir.next_correlated_event(include, clock))
        else {
            self.state = State::Failed;
            return Step::Failed;
        };
        if cancelled() {
            self.state = State::Failed;
            return match event {
                observation @ CorrelatedDirectoryEvent::Observation { .. } => {
                    Step::Returned(observation)
                }
                _ => Step::Failed,
            };
        }
        match event {
            CorrelatedDirectoryEvent::Progress => Step::Progress,
            CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            } if self.pending_admits.is_empty() => {
                self.state = State::Finished;
                Step::End {
                    event_watermark_exclusive: self.next_seq,
                    child_count_exclusive,
                }
            }
            CorrelatedDirectoryEvent::End { .. } => {
                self.state = State::Failed;
                Step::Failed
            }
            CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name,
                decision,
                ..
            } => {
                if decision == FilterDecision::Admit {
                    if self
                        .pending_admits
                        .iter()
                        .any(|pending| pending.child_id == child_id)
                        || self.pending_admits.len() == MAX_PENDING_ADMITS
                        || self.pending_name_bytes.saturating_add(raw_name.len())
                            > MAX_PENDING_NAME_BYTES
                    {
                        self.state = State::Failed;
                        return Step::Failed;
                    }
                    let start = self.pending_name_bytes;
                    let Some(end) = start.checked_add(raw_name.len()) else {
                        self.state = State::Failed;
                        return Step::Failed;
                    };
                    self.pending_names[start..end].copy_from_slice(&raw_name);
                    self.pending_name_bytes = end;
                    self.pending_admits.push(PendingAdmit {
                        child_id,
                        start,
                        len: raw_name.len(),
                    });
                }
                Step::Progress
            }
            CorrelatedDirectoryEvent::Observation { child_id, entry }
                if self.matches_callback(child_id, &entry.entry.name) =>
            {
                Step::Accepted {
                    observation: Some(QueuedEntry {
                        timed: entry,
                        child_id: Some(child_id),
                        child_attempt: None,
                        payload_permit: None,
                    }),
                }
            }
            CorrelatedDirectoryEvent::Observation { .. } => {
                self.state = State::Failed;
                Step::Failed
            }
        }
    }

    pub(in crate::scan::parallel) fn retain_event(&mut self, bytes: usize) -> Option<Permit> {
        let mut permit = self.event_permit.take()?;
        if !permit.resize(bytes) {
            return None;
        }
        Some(permit)
    }

    pub(in crate::scan::parallel) fn release_event(&mut self) {
        self.event_permit = None;
    }

    pub(in crate::scan::parallel) fn end_acknowledged(&mut self) {
        self.state = State::Finished;
        self.token = None;
        self.release_buffers();
    }

    pub(in crate::scan::parallel) fn cancel(&mut self) {
        self.state = State::Failed;
        self.token = None;
        self.release_buffers();
    }

    pub(in crate::scan::parallel) fn unselected(&self) -> bool {
        matches!(self.state, State::Unselected)
    }

    pub(in crate::scan::parallel) fn legacy(&self) -> bool {
        matches!(self.state, State::Legacy)
    }

    pub(in crate::scan::parallel) fn bind(&mut self, token: du_analysis_index::FolderAttemptToken) {
        self.token = Some(token);
    }

    pub(in crate::scan::parallel) fn token(&self) -> Option<du_analysis_index::FolderAttemptToken> {
        self.token
    }

    pub(in crate::scan::parallel) fn saw_callback(&self, child_id: u64) -> bool {
        self.pending_admits
            .iter()
            .any(|pending| pending.child_id == child_id)
    }

    pub(in crate::scan::parallel) fn matches_callback(&self, child_id: u64, name: &[u8]) -> bool {
        self.pending_admits.iter().any(|pending| {
            pending.child_id == child_id
                && self
                    .pending_names
                    .get(pending.start..pending.start + pending.len)
                    == Some(name)
        })
    }

    pub(in crate::scan::parallel) fn finish_child(&mut self, child_id: u64) {
        if let Some(index) = self
            .pending_admits
            .iter()
            .position(|pending| pending.child_id == child_id)
        {
            let pending = self.pending_admits.swap_remove(index);
            let end = pending.start + pending.len;
            self.pending_names
                .copy_within(end..self.pending_name_bytes, pending.start);
            self.pending_name_bytes -= pending.len;
            for item in &mut self.pending_admits {
                if item.start >= end {
                    item.start -= pending.len;
                }
            }
        }
    }

    #[cfg(test)]
    pub(in crate::scan::parallel) fn add_callback_for_test(&mut self, child_id: u64, name: &[u8]) {
        let start = self.pending_name_bytes;
        let end = start.saturating_add(name.len());
        if end > self.pending_names.len() {
            return;
        }
        self.pending_names[start..end].copy_from_slice(name);
        self.pending_name_bytes = end;
        self.pending_admits.push(PendingAdmit {
            child_id,
            start,
            len: name.len(),
        });
    }

    pub(in crate::scan::parallel) fn disable_evidence(&mut self) {
        self.token = None;
        self.state = State::Failed;
        self.event_permit = None;
        self.release_buffers();
    }

    /// Keeps reading an already selected adapter after optional evidence cannot be stored.
    pub(in crate::scan::parallel) fn evidence_failed(&mut self) {
        self.token = None;
        self.event_permit = None;
    }

    /// Releases a not-yet-selected child attempt so ordinary enumeration can proceed.
    pub(in crate::scan::parallel) fn fallback_to_unselected(&mut self) {
        self.release_buffers();
        self.token = None;
        self.next_seq = 0;
        self.state = State::Unselected;
    }

    pub(in crate::scan::parallel) fn fail_pending(&mut self) {
        self.token = None;
        self.state = State::Failed;
        self.event_permit = None;
    }

    pub(in crate::scan::parallel) fn final_sequence(&mut self) -> Option<u64> {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.checked_add(1)?;
        Some(seq)
    }

    fn release_buffers(&mut self) {
        self.pending_admits = Vec::new();
        self.pending_names = Vec::new();
        self.pending_name_bytes = 0;
        self.event_permit = None;
        self.attempt_permit = None;
        self.budget = None;
    }
}

const MAX_EVENT_RESERVATION: Option<usize> =
    match MAX_PENDING_NAME_BYTES.checked_add(MAX_EVENT_PAYLOAD_BYTES.saturating_mul(2)) {
        Some(bytes) => bytes.checked_add(size_of::<CorrelatedDirectoryEvent>()),
        None => None,
    };

#[cfg(test)]
mod tests;
