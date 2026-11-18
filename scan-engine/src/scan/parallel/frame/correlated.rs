//! Per-frame staging for an optional correlated enumeration.
//!
//! This boundary is not enabled until the result receiver owns its events.
mod next;

use super::{Directory, QueuedEntry};
use crate::scan::parallel::correlated_budget::{CorrelatedBudget, MAX_EVENT_PAYLOAD_BYTES, Permit};
#[cfg(test)]
use du_core_contracts::{Timestamp, scan_observation::FilterDecision};
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::CorrelatedDirectoryEvent,
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

    /// Keeps subsequent reads on the legacy path after an unsupported correlated selection.
    pub(in crate::scan::parallel) fn fallback_to_legacy(&mut self) {
        self.release_buffers();
        self.token = None;
        self.next_seq = 0;
        self.state = State::Legacy;
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
