//! Bounded handoff from scan admission to app-host's recent-scope worker.
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
        mpsc,
    },
};

use du_core_contracts::{
    Id,
    analysis_ipc::{ScanStart, ScopeDraft},
    scan_observation::{
        OpenedScopeRoot, OptionalRecentScopeObserver, RecentScopeEvidence, RecentScopeObservation,
        RecentScopeObserver,
    },
};

/// A newly admitted GUI scan and deferred observations of its original roots.
pub struct RecentScopeAdmission {
    pub receipt: ScanStart,
    pub normalized_scope: ScopeDraft,
    pub(crate) fingerprint_input: Option<ScopeDraft>,
    pub(crate) observers: Vec<RetainedRoot>,
    pub(crate) queue_slots: Option<Arc<AtomicUsize>>,
}

/// Binds an optional observer to the identity passed to the scan worker.
pub(crate) struct RetainedRoot {
    volume_id: Vec<u8>,
    file_id: Vec<u8>,
    observer: Box<dyn RecentScopeObserver>,
}

impl RetainedRoot {
    pub(crate) fn bind_all(
        roots: &[OpenedScopeRoot],
        observers: Vec<OptionalRecentScopeObserver>,
    ) -> Option<Vec<Self>> {
        if roots.is_empty() || roots.len() != observers.len() {
            return None;
        }
        roots
            .iter()
            .zip(observers)
            .map(|(root, observer)| {
                if root.volume_id.is_empty() || root.file_id.is_empty() {
                    return None;
                }
                Some(Self {
                    volume_id: root.volume_id.clone(),
                    file_id: root.file_id.clone(),
                    observer: observer?.ok()?,
                })
            })
            .collect()
    }

    fn observe(&self, cancelled: &AtomicBool) -> Option<RecentScopeEvidence> {
        match self.observer.observe(cancelled) {
            RecentScopeObservation::Observed(evidence)
                if evidence.volume_id == self.volume_id && evidence.file_id == self.file_id =>
            {
                Some(evidence)
            }
            _ => None,
        }
    }
}

impl RecentScopeAdmission {
    /// Returns the admitted scope inputs hashed for the receipt fingerprint.
    /// Consumers must not reconstruct comparison keys from display or applied paths.
    pub fn fingerprint_conditions(&self) -> &ScopeDraft {
        self.fingerprint_input
            .as_ref()
            .unwrap_or(&self.normalized_scope)
    }

    /// Collects same-root evidence for an admission released by successful GUI delivery.
    ///
    /// `cancelled` belongs to the recent-scope worker. The scan lifecycle is independent
    /// of this handoff. Recheck the current setting and worker cancellation at the
    /// persistence boundary before committing the evidence.
    pub fn observe(&self, cancelled: &AtomicBool) -> Option<Vec<RecentScopeEvidence>> {
        let mut evidence = Vec::with_capacity(self.observers.len());
        for observer in &self.observers {
            if cancelled.load(Ordering::Acquire) {
                return None;
            }
            evidence.push(observer.observe(cancelled)?);
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
    pending: VecDeque<QueuedAdmission>,
    receiver_open: bool,
}

const UNCLAIMED: u8 = 0;
const CLAIMED: u8 = 1;
const POSTED: u8 = 2;
const DISCARDED: u8 = 3;

struct QueuedAdmission {
    admission: RecentScopeAdmission,
    delivery: Arc<AtomicU8>,
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
            pending.pending.push_back(QueuedAdmission {
                admission,
                delivery: Arc::new(AtomicU8::new(UNCLAIMED)),
            });
        }
    }
}

/// Lets an IPC worker claim the delivery outcome for a new scan receipt.
#[derive(Clone)]
pub struct RecentScopeDelivery {
    pending: PendingAdmissions,
}

impl RecentScopeDelivery {
    /// Claims the delivery once, without transferring the scan's retained handles.
    pub fn claim(&self, job_ref: &Id) -> Option<RecentScopeDeliveryReceipt> {
        let queue = self.pending.lock().ok()?;
        let candidate = queue
            .pending
            .iter()
            .find(|item| &item.admission.receipt.job_ref == job_ref)?;
        candidate
            .delivery
            .compare_exchange(UNCLAIMED, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(RecentScopeDeliveryReceipt(candidate.delivery.clone()))
    }
}

/// A one-shot receipt whose completion can run on the WebView thread.
/// Dropping it without confirming delivery discards the candidate.
pub struct RecentScopeDeliveryReceipt(Arc<AtomicU8>);

impl RecentScopeDeliveryReceipt {
    /// Records that the GUI transport accepted the reply. This does no I/O.
    pub fn posted(self) {
        self.0.store(POSTED, Ordering::Release);
    }
}

impl Drop for RecentScopeDeliveryReceipt {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(CLAIMED, DISCARDED, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// Receiver owned by app-host's background recent-scope worker.
pub struct RecentScopeAdmissions {
    pending: PendingAdmissions,
}

impl RecentScopeAdmissions {
    /// Returns a delivery handle that can be passed to the IPC workers.
    pub fn delivery(&self) -> RecentScopeDelivery {
        RecentScopeDelivery {
            pending: self.pending.clone(),
        }
    }

    /// Discards undelivered candidates and retrieves one delivered admission.
    /// Call this off the GUI thread because discarding can close native handles.
    pub fn try_next(&self) -> Result<RecentScopeAdmission, mpsc::TryRecvError> {
        loop {
            let mut queue = self
                .pending
                .lock()
                .map_err(|_| mpsc::TryRecvError::Disconnected)?;
            let Some((index, posted)) =
                queue.pending.iter().enumerate().find_map(|(index, item)| {
                    match item.delivery.load(Ordering::Acquire) {
                        POSTED => Some((index, true)),
                        DISCARDED => Some((index, false)),
                        _ => None,
                    }
                })
            else {
                return Err(mpsc::TryRecvError::Empty);
            };
            let item = queue
                .pending
                .remove(index)
                .ok_or(mpsc::TryRecvError::Disconnected)?;
            drop(queue);
            if posted {
                return Ok(item.admission);
            }
        }
    }
}

impl Drop for RecentScopeAdmissions {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.pending.lock() {
            queue.receiver_open = false;
            // Dropping an observer can close a native handle. Release the queue first.
            let pending = std::mem::take(&mut queue.pending);
            drop(queue);
            drop(pending);
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

    struct Evidence(Vec<u8>, Vec<u8>);

    impl RecentScopeObserver for Evidence {
        fn observe(&self, _: &AtomicBool) -> RecentScopeObservation {
            use du_core_contracts::{OsPath, PathBytes, PathEncoding};
            let path = OsPath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/fixture"),
                display: "/fixture".into(),
                lossy: false,
            };
            RecentScopeObservation::Observed(RecentScopeEvidence {
                volume_id: self.0.clone(),
                file_id: self.1.clone(),
                entry_path: path,
                relative_path: OsPath {
                    encoding: PathEncoding::Unix,
                    raw: PathBytes::from_bytes(&[]),
                    display: String::new(),
                    lossy: false,
                },
            })
        }
    }

    #[test]
    fn observation_must_match_the_original_scan_volume_and_file() {
        let cancelled = AtomicBool::new(false);
        for (volume, file, accepted) in [(1, 2, true), (3, 2, false), (1, 3, false)] {
            let retained = RetainedRoot {
                volume_id: vec![1],
                file_id: vec![2],
                observer: Box::new(Evidence(vec![volume], vec![file])),
            };
            assert_eq!(retained.observe(&cancelled).is_some(), accepted);
        }
    }

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
