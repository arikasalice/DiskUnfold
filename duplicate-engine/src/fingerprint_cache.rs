//! Session-only cache of full content fingerprints, not duplicate verdicts.
//!
//! Selection, capabilities and time are supplied by the trusted composition
//! point. The cache is not a content-open authorization or a persistence format.
use crate::{
    RegisteredHash, RegisteredOutcome,
    candidates::{EntryKind, FileIdentity, Placeholder, Size},
    verification::{self, FullReuse, ReadPort, State, Strategies, VerificationTarget, VerifyError},
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

/// Trusted per-volume capability and time sources for one selected scope.
pub struct ReuseInputs<'a> {
    pub capabilities: &'a dyn Fn(&FileIdentity) -> Option<ReuseCapabilities>,
    pub clock: &'a dyn Fn() -> Option<i64>,
    /// True only when all comparison peers in the selected scope were observed.
    pub scope_proven: bool,
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
        reader: &mut (impl ReadPort + ?Sized),
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
        self.insert(key, digest, verified_at_ms)
    }

    /// Called only after the shared full-read path has verified this digest.
    pub(crate) fn record_checked(
        &self,
        target: &VerificationTarget,
        capabilities: ReuseCapabilities,
        hash: &RegisteredHash,
        digest: Vec<u8>,
        verified_at_ms: i64,
    ) -> Result<(), Error> {
        let key = key(target, capabilities, hash).ok_or(Error::UnavailableEvidence)?;
        self.insert(key, digest, verified_at_ms)
    }

    /// Runs the registered verifier with explicitly supplied session evidence.
    /// Capabilities must be fresh, trusted, and specific to the selected volume.
    /// # Errors
    /// Returns invalid-target or unsupported-strategy errors from verification.
    pub fn verify_with_reuse(
        &self,
        approved: &[VerificationTarget],
        reader: &mut impl ReadPort,
        hash: &RegisteredHash,
        inputs: &ReuseInputs<'_>,
        cancelled: impl FnMut() -> bool,
    ) -> Result<RegisteredOutcome, VerifyError> {
        let session = Session {
            cache: self,
            hash,
            capabilities: inputs.capabilities,
            clock: inputs.clock,
        };
        let strategy = hash.strategy();
        verification::verify_for_scope_with_reuse(
            approved,
            reader,
            Strategies {
                sample: strategy,
                full: strategy,
            },
            inputs.scope_proven,
            Some(&session),
            cancelled,
        )
        .map(RegisteredOutcome::from_verified)
    }

    fn insert(&self, key: Key, digest: Vec<u8>, verified_at_ms: i64) -> Result<(), Error> {
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

struct Session<'a> {
    cache: &'a FingerprintCache,
    hash: &'a RegisteredHash,
    capabilities: &'a dyn Fn(&FileIdentity) -> Option<ReuseCapabilities>,
    clock: &'a dyn Fn() -> Option<i64>,
}

impl FullReuse for Session<'_> {
    fn lookup(
        &self,
        target: &VerificationTarget,
        reader: &mut dyn ReadPort,
    ) -> Option<(Vec<u8>, i64)> {
        let now = (self.clock)()?;
        let capabilities = (self.capabilities)(target.target.identity.as_ref()?)?;
        match self
            .cache
            .decide(target, capabilities, self.hash, reader)
            .ok()?
        {
            Decision::Reuse(fingerprint) if now >= fingerprint.verified_at_ms => {
                Some((fingerprint.digest, fingerprint.verified_at_ms))
            }
            Decision::Reuse(_) => None,
            Decision::ReadRequired => None,
        }
    }
    fn record(&self, target: &VerificationTarget, digest: &[u8]) -> Option<i64> {
        let time = (self.clock)()?;
        if let Some(capabilities) = target.target.identity.as_ref().and_then(self.capabilities) {
            let _ =
                self.cache
                    .record_checked(target, capabilities, self.hash, digest.to_vec(), time);
        }
        Some(time)
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
