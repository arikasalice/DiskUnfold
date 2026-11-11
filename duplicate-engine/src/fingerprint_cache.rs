//! Session-only cache of full content fingerprints, not duplicate verdicts.
//!
//! Selection, capabilities and time are supplied by the trusted composition
//! point. The cache is not a content-open authorization or a persistence format.
use crate::{
    RegisteredHash,
    candidates::{EntryKind, FileIdentity, Placeholder, Size},
    verification::{ReadPort, State, VerificationTarget},
};
use std::{collections::BTreeMap, num::NonZeroUsize, sync::Mutex, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    identity: FileIdentity,
    size: u64,
    modified: u64,
    change_stamp: u64,
    algorithm: String,
    revision: u32,
}

/// Capabilities observed for the same volume as the selected file.
#[derive(Clone, Copy, Debug)]
pub struct ReuseCapabilities {
    /// True only when the adapter established a reliable change stamp.
    pub change_stamp_supported: bool,
    /// Effective modification-time resolution, or None when unknown.
    pub modified_resolution: Option<Duration>,
}

/// Reused full fingerprint and the time of its original successful read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReusedFingerprint {
    pub digest: Vec<u8>,
    pub verified_at_ms: i64,
}

/// A cache miss is always a request to perform a fresh read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    ReadRequired,
    Reuse(ReusedFingerprint),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Input does not establish reuse-capable identity, metadata and capabilities.
    UnavailableEvidence,
    /// Identity-bound content verification failed or changed.
    ReadFailed(State),
    /// Cancellation prevented publication of the fingerprint.
    Cancelled,
    /// No trusted verification time was available after the read.
    ClockUnavailable,
    /// The caller-selected bound is full. No entry was evicted.
    Capacity,
    /// The cache lock is unavailable.
    Unavailable,
}

/// Explicitly bounded session cache. It never selects a retention or eviction policy.
pub struct FingerprintCache {
    entries: Mutex<BTreeMap<Key, ReusedFingerprint>>,
    capacity: NonZeroUsize,
}

impl FingerprintCache {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
            capacity,
        }
    }

    /// Uses only a complete match; unknown values or missing records require reading.
    /// # Errors
    /// Returns `Unavailable` if the session lock cannot be acquired.
    pub fn decide(
        &self,
        target: &VerificationTarget,
        capabilities: ReuseCapabilities,
        hash: &RegisteredHash,
        reader: &mut impl ReadPort,
    ) -> Result<Decision, Error> {
        let Some(key) = key(target, capabilities, hash) else {
            return Ok(Decision::ReadRequired);
        };
        let cached = self
            .entries
            .lock()
            .map_err(|_| Error::Unavailable)?
            .get(&key)
            .cloned();
        let Some(cached) = cached else {
            return Ok(Decision::ReadRequired);
        };
        let Ok(mut handle) = reader.open_bound(target.target.item_no, &key.identity) else {
            return Ok(Decision::ReadRequired);
        };
        if handle.snapshot().ok().as_ref() != target.snapshot.as_ref()
            || handle.snapshot().ok().as_ref() != target.snapshot.as_ref()
        {
            return Ok(Decision::ReadRequired);
        }
        Ok(Decision::Reuse(cached))
    }

    /// Reads and checks one whole file before admitting a fingerprint.
    /// This never imports a digest or a duplicate-group verdict from a caller.
    /// A failed or cancelled read leaves the cache unchanged. The clock callback
    /// supplies UTC milliseconds after the read and its metadata checks.
    /// # Errors
    /// Returns an error for incomplete evidence, read failure, cancellation, capacity,
    /// or an unavailable lock. A full cache never evicts another entry.
    pub fn verify_and_record(
        &self,
        target: &VerificationTarget,
        capabilities: ReuseCapabilities,
        hash: &RegisteredHash,
        reader: &mut impl ReadPort,
        clock: impl FnOnce() -> Option<i64>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<(), Error> {
        let key = key(target, capabilities, hash).ok_or(Error::UnavailableEvidence)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let digest = hash
            .verified_full_for_reuse(target, reader)
            .map_err(Error::ReadFailed)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let verified_at_ms = clock().ok_or(Error::ClockUnavailable)?;
        let mut entries = self.entries.lock().map_err(|_| Error::Unavailable)?;
        if let Some(existing) = entries.get(&key) {
            if existing.verified_at_ms >= verified_at_ms {
                return Ok(());
            }
        } else if entries.len() >= self.capacity.get() {
            return Err(Error::Capacity);
        }
        entries.insert(
            key,
            ReusedFingerprint {
                digest,
                verified_at_ms,
            },
        );
        Ok(())
    }
}

fn key(target: &VerificationTarget, caps: ReuseCapabilities, hash: &RegisteredHash) -> Option<Key> {
    if !caps.change_stamp_supported
        || !matches!(caps.modified_resolution, Some(value) if value > Duration::ZERO && value <= Duration::from_secs(2))
        || target.target.kind != EntryKind::File
        || target.target.placeholder != Placeholder::Local
    {
        return None;
    }
    let Size::Exact(size) = target.target.size else {
        return None;
    };
    let snapshot = target.snapshot.as_ref()?;
    let identity = target.target.identity.as_ref()?;
    if identity.volume.is_empty()
        || identity.file.is_empty()
        || *identity != snapshot.identity
        || size != snapshot.size
    {
        return None;
    }
    let (algorithm, revision) = hash.reuse_identity();
    Some(Key {
        identity: identity.clone(),
        size,
        modified: snapshot.modified,
        change_stamp: snapshot.change_stamp?,
        algorithm: algorithm.into(),
        revision,
    })
}
