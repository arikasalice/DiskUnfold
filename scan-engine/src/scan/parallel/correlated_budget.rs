//! Per-scan limits for memory owned by correlated directory collection.
use du_core_contracts::{
    ports::PortError,
    scan_observation::{
        CorrelatedDirectoryEvent, MetadataObservation, ObservedEntry, SpaceIdentity,
        TimedObservedEntry,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

pub(super) const MAX_ATTEMPTS: usize = 256;
pub(super) const MAX_MANAGED_BYTES: usize = 96 * 1024 * 1024;
pub(super) const MAX_EVENT_PAYLOAD_BYTES: usize = 256 * 1024;
static NEXT_ATTEMPT_ID: AtomicU64 = AtomicU64::new(1);

pub(super) fn next_attempt_id() -> Option<u64> {
    NEXT_ATTEMPT_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .ok()
}

pub(super) struct CorrelatedBudget {
    attempts: AtomicUsize,
    bytes: AtomicUsize,
    disabled: AtomicBool,
    max_attempts: usize,
    max_bytes: usize,
}

impl CorrelatedBudget {
    pub(super) fn new() -> Arc<Self> {
        Self::with_limits(MAX_ATTEMPTS, MAX_MANAGED_BYTES)
    }

    pub(super) fn with_limits(max_attempts: usize, max_bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            attempts: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            disabled: AtomicBool::new(false),
            max_attempts,
            max_bytes,
        })
    }

    pub(super) fn reserve_attempt(self: &Arc<Self>, bytes: usize) -> Option<Permit> {
        if self.disabled.load(Ordering::Acquire) {
            return None;
        }
        if !reserve_counter(&self.attempts, self.max_attempts, 1) {
            return None;
        }
        if !reserve_counter(&self.bytes, self.max_bytes, bytes) {
            self.attempts.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Permit {
            budget: Arc::clone(self),
            bytes,
            attempt_slot: true,
        })
    }

    pub(super) fn reserve(self: &Arc<Self>, bytes: usize) -> Option<Permit> {
        if self.disabled.load(Ordering::Acquire) {
            return None;
        }
        reserve_counter(&self.bytes, self.max_bytes, bytes).then(|| Permit {
            budget: Arc::clone(self),
            bytes,
            attempt_slot: false,
        })
    }

    pub(super) fn reserve_channel(self: &Arc<Self>, bytes: usize) -> Option<Permit> {
        let permit = self.reserve(bytes);
        if permit.is_none() {
            self.disabled.store(true, Ordering::Release);
        }
        permit
    }

    #[cfg(test)]
    fn used(&self) -> (usize, usize) {
        (
            self.attempts.load(Ordering::Acquire),
            self.bytes.load(Ordering::Acquire),
        )
    }

    #[cfg(test)]
    fn is_disabled(&self) -> bool {
        self.disabled.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn disable_for_test(&self) {
        self.disabled.store(true, Ordering::Release);
    }
}

fn reserve_counter(counter: &AtomicUsize, limit: usize, amount: usize) -> bool {
    counter
        .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
            used.checked_add(amount).filter(|next| *next <= limit)
        })
        .is_ok()
}

/// A reservation held for as long as the corresponding correlated allocation can be retained.
pub(super) struct Permit {
    budget: Arc<CorrelatedBudget>,
    bytes: usize,
    attempt_slot: bool,
}

pub(super) fn event_payload_capacity(event: &CorrelatedDirectoryEvent) -> Option<usize> {
    match event {
        CorrelatedDirectoryEvent::FilterCallback { raw_name, .. } => Some(raw_name.capacity()),
        CorrelatedDirectoryEvent::Observation { entry, .. } => timed_heap_capacity(entry),
        CorrelatedDirectoryEvent::Progress | CorrelatedDirectoryEvent::End { .. } => Some(0),
    }
}

pub(super) fn path_heap_capacity(path: &[Vec<u8>], path_capacity: usize) -> Option<usize> {
    let segments = path_capacity.checked_mul(std::mem::size_of::<Vec<u8>>())?;
    path.iter().try_fold(segments, |bytes, segment| {
        bytes.checked_add(segment.capacity())
    })
}

pub(super) fn path_clone_reservation(path: &[Vec<u8>]) -> Option<usize> {
    let segments = path.len().checked_mul(std::mem::size_of::<Vec<u8>>())?;
    path.iter()
        .try_fold(segments, |bytes, segment| bytes.checked_add(segment.len()))
}

pub(super) fn child_path_clone_reservation(path: &[Vec<u8>], name: &[u8]) -> Option<usize> {
    path_clone_reservation(path)?
        .checked_add(std::mem::size_of::<Vec<u8>>())?
        .checked_add(name.len())
}

pub(super) fn try_clone_path(path: &[Vec<u8>]) -> Option<Vec<Vec<u8>>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(path.len()).ok()?;
    for segment in path {
        copy.push(try_clone_bytes(segment)?);
    }
    Some(copy)
}

pub(super) fn try_clone_child_path(path: &[Vec<u8>], name: &[u8]) -> Option<Vec<Vec<u8>>> {
    let mut copy = try_clone_path(path)?;
    copy.try_reserve_exact(1).ok()?;
    copy.push(try_clone_bytes(name)?);
    Some(copy)
}

pub(super) fn timed_heap_capacity(entry: &TimedObservedEntry) -> Option<usize> {
    entry_heap_capacity(&entry.entry)
}

pub(super) fn entry_heap_capacity(entry: &ObservedEntry) -> Option<usize> {
    let mut bytes = entry.name.capacity();
    if let Err(error) = &entry.metadata {
        bytes = bytes.checked_add(error.reason_key.capacity())?;
    } else if let Ok(metadata) = &entry.metadata {
        if let Err(error) = &metadata.size {
            bytes = bytes.checked_add(error.reason_key.capacity())?;
        }
        if let Some(identity) = &metadata.space_identity {
            bytes = bytes
                .checked_add(identity.volume.capacity())?
                .checked_add(identity.file.capacity())?;
        }
    }
    Some(bytes)
}

/// Fallible owned copy used when F02 must keep an observation beside the F03 event.
pub(super) fn try_clone_timed(entry: &TimedObservedEntry) -> Option<TimedObservedEntry> {
    Some(TimedObservedEntry {
        entry: try_clone_entry(&entry.entry)?,
        observed_at: entry.observed_at.clone(),
        placeholder_observation: entry.placeholder_observation,
    })
}

pub(super) fn try_clone_entry(entry: &ObservedEntry) -> Option<ObservedEntry> {
    let metadata = match &entry.metadata {
        Ok(metadata) => Ok(MetadataObservation {
            kind: metadata.kind.clone(),
            device: metadata.device,
            inode: metadata.inode,
            size: match &metadata.size {
                Ok(size) => Ok(*size),
                Err(error) => Err(try_clone_port_error(error)?),
            },
            modified_at: metadata.modified_at.clone(),
            native_modified_at: metadata.native_modified_at.clone(),
            allocated: metadata.allocated,
            link_count: metadata.link_count,
            space_identity: match &metadata.space_identity {
                Some(identity) => Some(try_clone_space_identity(identity)?),
                None => None,
            },
        }),
        Err(error) => Err(try_clone_port_error(error)?),
    };
    Some(ObservedEntry {
        name: try_clone_bytes(&entry.name)?,
        enumerated_inode: entry.enumerated_inode,
        enumerated_kind: entry.enumerated_kind.clone(),
        metadata,
    })
}

fn try_clone_port_error(error: &PortError) -> Option<PortError> {
    Some(PortError {
        kind: error.kind.clone(),
        reason_key: try_clone_string(&error.reason_key)?,
        os_code: error.os_code,
    })
}

fn try_clone_space_identity(identity: &SpaceIdentity) -> Option<SpaceIdentity> {
    Some(SpaceIdentity {
        volume: try_clone_bytes(&identity.volume)?,
        file: try_clone_bytes(&identity.file)?,
    })
}

fn try_clone_bytes(source: &[u8]) -> Option<Vec<u8>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(source.len()).ok()?;
    copy.extend_from_slice(source);
    Some(copy)
}

fn try_clone_string(source: &str) -> Option<String> {
    let mut copy = String::new();
    copy.try_reserve_exact(source.len()).ok()?;
    copy.push_str(source);
    Some(copy)
}

impl Permit {
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Grows before the caller allocates, or shrinks after the retained capacity is known.
    pub(super) fn resize(&mut self, bytes: usize) -> bool {
        if bytes > self.bytes {
            if !reserve_counter(
                &self.budget.bytes,
                self.budget.max_bytes,
                bytes - self.bytes,
            ) {
                return false;
            }
        } else {
            self.budget
                .bytes
                .fetch_sub(self.bytes - bytes, Ordering::AcqRel);
        }
        self.bytes = bytes;
        true
    }

    pub(super) fn add(&mut self, bytes: usize) -> bool {
        let Some(next) = self.bytes.checked_add(bytes) else {
            return false;
        };
        self.resize(next)
    }

    /// Combines same-scan reservations without releasing either charge in between.
    pub(super) fn merge(&mut self, mut other: Permit) -> bool {
        if self.attempt_slot || other.attempt_slot || !Arc::ptr_eq(&self.budget, &other.budget) {
            return false;
        }
        let Some(bytes) = self.bytes.checked_add(other.bytes) else {
            return false;
        };
        self.bytes = bytes;
        other.bytes = 0;
        true
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.budget.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
        if self.attempt_slot {
            self.budget.attempts.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_and_bytes_are_bounded_and_released() {
        let budget = CorrelatedBudget::with_limits(1, 10);
        let mut first = budget.reserve_attempt(6);
        assert!(first.is_some());
        let Some(first) = first.take() else {
            return;
        };
        assert!(budget.reserve_attempt(1).is_none());
        assert!(budget.reserve(5).is_none());
        drop(first);
        assert_eq!(budget.used(), (0, 0));
        let last = budget.reserve_attempt(10);
        assert!(last.is_some());
    }

    #[test]
    fn resize_reserves_before_growth_and_releases_on_shrink() {
        let budget = CorrelatedBudget::with_limits(1, 12);
        let mut permit = budget.reserve_attempt(8);
        assert!(permit.is_some());
        let Some(mut permit) = permit.take() else {
            return;
        };
        assert!(!permit.resize(13));
        assert_eq!(permit.bytes(), 8);
        assert!(permit.resize(12));
        assert!(permit.resize(4));
        assert_eq!(budget.used(), (1, 4));
        drop(permit);
        assert_eq!(budget.used(), (0, 0));
    }

    #[test]
    fn channel_reservation_failure_disables_evidence_without_scan_failure() {
        let budget = CorrelatedBudget::with_limits(1, 1);
        assert!(budget.reserve_channel(2).is_none());
        assert!(budget.is_disabled());
        assert!(budget.reserve_attempt(1).is_none());
    }
}
