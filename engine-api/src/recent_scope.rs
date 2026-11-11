//! Bounded handoff from scan admission to app-host's recent-scope worker.
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
};

use du_core_contracts::{
    analysis_ipc::{ScanStart, ScopeDraft},
    scan_observation::{RecentScopeEvidence, RecentScopeObservation},
};

/// A newly admitted GUI scan and same-open deferred root observations.
pub struct RecentScopeAdmission {
    pub receipt: ScanStart,
    pub normalized_scope: ScopeDraft,
    pub(crate) observers: Vec<du_core_contracts::scan_observation::OptionalRecentScopeObserver>,
    pub(crate) runtime: Arc<du_job_runtime::JobRuntime>,
    pub(crate) queue_slots: Option<Arc<AtomicUsize>>,
}

/// Authoritative job lifecycle view for the app-host admission consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecentScopeAdmissionProgress {
    Running,
    Ready,
    TerminalWithoutEvidence,
}

impl RecentScopeAdmission {
    /// Lets the app-host retain a candidate while its owning job is running.
    pub fn progress(&self) -> RecentScopeAdmissionProgress {
        match self.runtime.state(&self.receipt.job_ref) {
            Ok(
                du_core_contracts::job::JobState::Done | du_core_contracts::job::JobState::Partial,
            ) => RecentScopeAdmissionProgress::Ready,
            Ok(state) if !state.is_terminal() => RecentScopeAdmissionProgress::Running,
            Ok(_) | Err(_) => RecentScopeAdmissionProgress::TerminalWithoutEvidence,
        }
    }

    /// Collects evidence only after the owning job runtime reports this job done or partial.
    ///
    /// This returns candidate evidence, not authorization to persist it. Callers
    /// must check cancellation again at the persistence boundary. Failed-job
    /// recent-scope semantics remain unimplemented pending a design decision.
    pub fn observe(&self, cancelled: &AtomicBool) -> Option<Vec<RecentScopeEvidence>> {
        if self.progress() != RecentScopeAdmissionProgress::Ready {
            return None;
        }
        let mut evidence = Vec::with_capacity(self.observers.len());
        for observer in &self.observers {
            if cancelled.load(Ordering::Acquire) {
                return None;
            }
            let observer = observer.as_ref()?.as_ref().ok()?;
            match observer.observe(cancelled) {
                RecentScopeObservation::Observed(item) => evidence.push(item),
                RecentScopeObservation::Cancelled | RecentScopeObservation::Unavailable(_) => {
                    return None;
                }
            }
        }
        (!cancelled.load(Ordering::Acquire) && !evidence.is_empty()).then_some(evidence)
    }
}

impl Drop for RecentScopeAdmission {
    fn drop(&mut self) {
        if let Some(slots) = self.queue_slots.take() {
            slots.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

#[derive(Default)]
struct AdmissionQueue {
    pending: VecDeque<RecentScopeAdmission>,
    ready: VecDeque<RecentScopeAdmission>,
    receiver_open: bool,
}

type PendingAdmissions = Arc<Mutex<AdmissionQueue>>;

/// Publisher owned by the scan admission use case.
#[derive(Clone)]
pub(crate) struct RecentScopeAdmissionPublisher(PendingAdmissions, usize, Arc<AtomicUsize>);

impl RecentScopeAdmissionPublisher {
    pub(crate) fn enqueue(&self, admission: RecentScopeAdmission) {
        if let Ok(mut pending) = self.0.lock()
            && pending.receiver_open
            && self
                .2
                .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < self.1).then(|| count + 1)
                })
                .is_ok()
        {
            let mut admission = admission;
            admission.queue_slots = Some(self.2.clone());
            pending.pending.push_back(admission);
        }
    }
}

/// Receiver consumed by app-host's background recent-scope worker.
pub struct RecentScopeAdmissions {
    pending: PendingAdmissions,
}

impl RecentScopeAdmissions {
    /// Releases deferred evidence only after app-host has returned the scan receipt.
    pub fn response_sent(&self, job_ref: &du_core_contracts::Id) -> bool {
        let Ok(mut queue) = self.pending.lock() else {
            return false;
        };
        if !queue.receiver_open {
            return false;
        }
        let Some(index) = queue
            .pending
            .iter()
            .position(|item| &item.receipt.job_ref == job_ref)
        else {
            return false;
        };
        let Some(admission) = queue.pending.remove(index) else {
            return false;
        };
        queue.ready.push_back(admission);
        true
    }

    /// Drops an admission cancelled before its scan receipt was returned.
    pub fn cancel_before_response(&self, job_ref: &du_core_contracts::Id) -> bool {
        let Ok(mut queue) = self.pending.lock() else {
            return false;
        };
        let Some(index) = queue
            .pending
            .iter()
            .position(|item| &item.receipt.job_ref == job_ref)
        else {
            return false;
        };
        queue.pending.remove(index).is_some()
    }

    /// Retrieves one admission without waiting for another scan.
    pub fn try_next(&self) -> Result<RecentScopeAdmission, mpsc::TryRecvError> {
        let Ok(mut queue) = self.pending.lock() else {
            return Err(mpsc::TryRecvError::Disconnected);
        };
        if let Some(admission) = queue.ready.pop_front() {
            return Ok(admission);
        }
        if queue.receiver_open {
            Err(mpsc::TryRecvError::Empty)
        } else {
            Err(mpsc::TryRecvError::Disconnected)
        }
    }
}

impl Drop for RecentScopeAdmissions {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.pending.lock() {
            queue.receiver_open = false;
            queue.pending.clear();
            queue.ready.clear();
        }
    }
}

pub(crate) fn recent_scope_admissions(
    capacity: usize,
) -> (RecentScopeAdmissionPublisher, RecentScopeAdmissions) {
    let pending = Arc::new(Mutex::new(AdmissionQueue {
        receiver_open: true,
        ..AdmissionQueue::default()
    }));
    (
        RecentScopeAdmissionPublisher(pending.clone(), capacity, Arc::new(AtomicUsize::new(0))),
        RecentScopeAdmissions { pending },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_counter_with_maximum_capacity_does_not_overflow() {
        let counter = AtomicUsize::new(usize::MAX);
        let result = counter.try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < usize::MAX).then(|| count + 1)
        });

        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::Acquire), usize::MAX);
    }
}
