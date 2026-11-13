//! Per-window query regions. The registry does not invoke the job runtime.
mod cancellation;
use du_core_contracts::{Id, ports::Random};
use du_job_runtime::JobIdentity;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Authenticated identity of the owning session and window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub session: Id,
    pub window: Id,
}

/// Internal failures to be mapped by the transport owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionError {
    Unavailable,
    WrongKind,
    InvalidSequence,
    Capacity,
    Entropy,
    Poisoned,
}

/// A cancellation decision. Invoke F24 only after this method has returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CancelAction {
    Accepted,
    NoEffect,
    Job(Id),
}

struct Current {
    sequence: u32,
    stopped: Arc<AtomicBool>,
    finished: bool,
    published: bool,
    job: Option<Id>,
}
struct Region<K> {
    kind: K,
    closed: bool,
    last_sequence: u32,
    current: Option<Current>,
}
struct Table<K> {
    closed: bool,
    issued: u64,
    regions: HashMap<Id, Arc<Mutex<Region<K>>>>,
}

/// One registry for one authenticated window session. `K` is the wire kind.
///
/// The owner must retain this instance until session retirement. References use
/// UUIDv7 timestamps from the injected identity source and a CSPRNG-seeded,
/// non-wrapping counter. Within this instance, references never repeat even
/// if the clock moves backward. Across instances, separation relies on 74 bits
/// of independently generated OS entropy, as with random UUIDs; owner checks
/// are still required and references are not authentication credentials.
/// The session owner must not replace a live registry or reuse a retired owner
/// identity; `random` must be backed by a cryptographically secure OS source.
pub struct RegionRegistry<K> {
    owner: Owner,
    limit: usize,
    seed: [u8; 10],
    ids: Arc<dyn JobIdentity>,
    stopped: Arc<AtomicBool>,
    table: Mutex<Table<K>>,
}

/// Proof that a validated request won promotion in a live region.
pub struct Permit<K> {
    region: Arc<Mutex<Region<K>>>,
    sequence: u32,
    stopped: Arc<AtomicBool>,
    session_stopped: Arc<AtomicBool>,
}
impl<K> Clone for Permit<K> {
    fn clone(&self) -> Self {
        Self {
            region: self.region.clone(),
            sequence: self.sequence,
            stopped: self.stopped.clone(),
            session_stopped: self.session_stopped.clone(),
        }
    }
}

/// A promoted request and the precise replaced job, if one existed.
pub struct Promotion<K, T> {
    pub permit: Permit<K>,
    pub previous_job: Option<Id>,
    pub prepared: T,
}

impl<K: Copy + Eq> RegionRegistry<K> {
    /// Creates a registry for exactly one window session using secure randomness.
    pub fn new(
        owner: Owner,
        limit: usize,
        ids: Arc<dyn JobIdentity>,
        random: &dyn Random,
    ) -> Result<Self, RegionError> {
        let mut seed = [0u8; 10];
        random.fill(&mut seed).map_err(|_| RegionError::Entropy)?;
        Ok(Self {
            owner,
            limit,
            seed,
            ids,
            stopped: Arc::new(AtomicBool::new(false)),
            table: Mutex::new(Table {
                closed: false,
                issued: 0,
                regions: HashMap::new(),
            }),
        })
    }
    fn owner(&self, owner: &Owner) -> Result<(), RegionError> {
        if *owner != self.owner || self.stopped.load(Ordering::Acquire) {
            Err(RegionError::Unavailable)
        } else {
            Ok(())
        }
    }
    /// Issues a fresh reference; callers never supply or reactivate a region ID.
    pub fn open(&self, owner: &Owner, kind: K) -> Result<Id, RegionError> {
        self.owner(owner)?;
        // The injected identity source can read clocks or entropy; never call it under the table lock.
        let candidate = self.ids.next_id();
        let parsed =
            uuid::Uuid::parse_str(candidate.as_str()).map_err(|_| RegionError::Unavailable)?;
        let mut table = self.table.lock().map_err(|_| RegionError::Poisoned)?;
        if table.closed {
            return Err(RegionError::Unavailable);
        }
        if table.regions.len() >= self.limit {
            return Err(RegionError::Capacity);
        }
        let sequence = table
            .issued
            .checked_add(1)
            .filter(|v| *v <= 0x3fff_ffff_ffff_ffff)
            .ok_or(RegionError::Capacity)?;
        let mut bytes = *parsed.as_bytes();
        bytes[6] = 0x70 | (self.seed[0] & 0x0f);
        bytes[7] = self.seed[1];
        let count = sequence.to_be_bytes();
        bytes[8] = 0x80 | ((self.seed[2] ^ count[0]) & 0x3f);
        for i in 0..7 {
            bytes[9 + i] = self.seed[3 + i] ^ count[1 + i];
        }
        let reference = Id::from_v7_bytes(bytes);
        table.issued = sequence;
        table.regions.insert(
            reference.clone(),
            Arc::new(Mutex::new(Region {
                kind,
                closed: false,
                last_sequence: 0,
                current: None,
            })),
        );
        Ok(reference)
    }
    fn lookup(&self, owner: &Owner, id: &Id) -> Result<Arc<Mutex<Region<K>>>, RegionError> {
        self.owner(owner)?;
        let table = self.table.lock().map_err(|_| RegionError::Poisoned)?;
        if table.closed {
            return Err(RegionError::Unavailable);
        }
        table
            .regions
            .get(id)
            .cloned()
            .ok_or(RegionError::Unavailable)
    }
    /// Checks the window session and operation family without changing a request.
    pub fn validate_region(&self, owner: &Owner, id: &Id, kind: K) -> Result<(), RegionError> {
        let entry = self.lookup(owner, id)?;
        let state = entry.lock().map_err(|_| RegionError::Poisoned)?;
        self.check(&state)?;
        if state.kind != kind {
            return Err(RegionError::WrongKind);
        }
        Ok(())
    }
    /// Atomically promotes a fully validated request; validation errors change nothing.
    pub fn promote_validated<T, E>(
        &self,
        owner: &Owner,
        id: &Id,
        kind: K,
        sequence: u32,
        validated: Result<T, E>,
    ) -> Result<Result<Promotion<K, T>, E>, RegionError> {
        self.validate_region(owner, id, kind)?;
        let prepared = match validated {
            Ok(value) => value,
            Err(error) => return Ok(Err(error)),
        };
        let entry = self.lookup(owner, id)?;
        let mut region = entry.lock().map_err(|_| RegionError::Poisoned)?;
        self.check(&region)?;
        if region.kind != kind {
            return Err(RegionError::WrongKind);
        }
        if sequence == 0 || sequence <= region.last_sequence {
            return Err(RegionError::InvalidSequence);
        }
        let previous_job = region.current.take().and_then(|old| {
            old.stopped.store(true, Ordering::Release);
            old.job
        });
        let stopped = Arc::new(AtomicBool::new(false));
        region.last_sequence = sequence;
        region.current = Some(Current {
            sequence,
            stopped: stopped.clone(),
            finished: false,
            published: false,
            job: None,
        });
        drop(region);
        Ok(Ok(Promotion {
            permit: Permit {
                region: entry,
                sequence,
                stopped,
                session_stopped: self.stopped.clone(),
            },
            previous_job,
            prepared,
        }))
    }
    /// Binds an admitted F24 job while promoting the region in one critical section.
    /// The caller must cancel the returned previous job only after releasing runtime locks.
    pub(crate) fn promote_job(
        &self,
        owner: &Owner,
        id: &Id,
        kind: K,
        sequence: u32,
        job: &Id,
    ) -> Result<(Permit<K>, Option<Id>), RegionError> {
        let entry = self.lookup(owner, id)?;
        let mut region = entry.lock().map_err(|_| RegionError::Poisoned)?;
        self.check(&region)?;
        if region.kind != kind {
            return Err(RegionError::WrongKind);
        }
        if sequence == 0 || sequence <= region.last_sequence {
            return Err(RegionError::InvalidSequence);
        }
        let previous_job = region.current.take().and_then(|old| {
            old.stopped.store(true, Ordering::Release);
            old.job
        });
        let stopped = Arc::new(AtomicBool::new(false));
        region.last_sequence = sequence;
        region.current = Some(Current {
            sequence,
            stopped: stopped.clone(),
            finished: false,
            published: false,
            job: Some(job.clone()),
        });
        drop(region);
        Ok((
            Permit {
                region: entry,
                sequence,
                stopped,
                session_stopped: self.stopped.clone(),
            },
            previous_job,
        ))
    }

    /// Promotes only while the caller remains active. Lock order matches the
    /// synchronous publication boundary: region first, then caller cancellation.
    pub(crate) fn promote_active(
        &self,
        owner: &Owner,
        id: &Id,
        kind: K,
        sequence: u32,
        caller: &crate::contracts::CallerCtx,
    ) -> Result<Option<Promotion<K, ()>>, RegionError> {
        let entry = self.lookup(owner, id)?;
        let mut region = entry.lock().map_err(|_| RegionError::Poisoned)?;
        self.check(&region)?;
        if region.kind != kind {
            return Err(RegionError::WrongKind);
        }
        if sequence == 0 || sequence <= region.last_sequence {
            return Err(RegionError::InvalidSequence);
        }
        let admitted = caller.with_active(|| {
            let previous = region.current.take().and_then(|old| {
                old.stopped.store(true, Ordering::Release);
                old.job
            });
            let stopped = Arc::new(AtomicBool::new(false));
            region.last_sequence = sequence;
            region.current = Some(Current {
                sequence,
                stopped: stopped.clone(),
                finished: false,
                published: false,
                job: None,
            });
            (stopped, previous)
        });
        drop(region);
        Ok(admitted.map(|(stopped, previous)| Promotion {
            permit: Permit {
                region: entry,
                sequence,
                stopped,
                session_stopped: self.stopped.clone(),
            },
            previous_job: previous,
            prepared: (),
        }))
    }
    fn check(&self, region: &Region<K>) -> Result<(), RegionError> {
        if region.closed || self.stopped.load(Ordering::Acquire) {
            Err(RegionError::Unavailable)
        } else {
            Ok(())
        }
    }
    /// Retires one reference and returns its exact job without waiting for it.
    pub fn close(&self, owner: &Owner, id: &Id) -> Result<Option<Id>, RegionError> {
        let entry = self.lookup(owner, id)?;
        let job = {
            let mut region = entry.lock().map_err(|_| RegionError::Poisoned)?;
            self.check(&region)?;
            region.closed = true;
            stop(&mut region)
        };
        let mut table = self.table.lock().map_err(|_| RegionError::Poisoned)?;
        if table
            .regions
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, &entry))
        {
            table.regions.remove(id);
            table.regions.shrink_to_fit();
        }
        Ok(job)
    }
    /// Retires the owner session and returns its jobs without waiting for them.
    pub fn close_session(&self, owner: &Owner) -> Result<Vec<Id>, RegionError> {
        self.owner(owner)?;
        let entries = {
            let mut table = self.table.lock().map_err(|_| RegionError::Poisoned)?;
            if table.closed {
                return Err(RegionError::Unavailable);
            }
            table.closed = true;
            self.stopped.store(true, Ordering::Release);
            std::mem::take(&mut table.regions)
        };
        let mut jobs = Vec::new();
        for entry in entries.into_values() {
            let mut region = entry.lock().map_err(|_| RegionError::Poisoned)?;
            if !region.closed {
                region.closed = true;
                if let Some(job) = stop(&mut region) {
                    jobs.push(job);
                }
            }
        }
        Ok(jobs)
    }
}
fn stop<K>(region: &mut Region<K>) -> Option<Id> {
    region.current.as_ref().and_then(|current| {
        current.stopped.store(true, Ordering::Release);
        current.job.clone()
    })
}

impl<K> Permit<K> {
    /// Indicates cancellation, replacement, or retirement of the window session.
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire) || self.session_stopped.load(Ordering::Acquire)
    }
    fn current_mut<'a>(&self, region: &'a mut Region<K>) -> Result<&'a mut Current, RegionError> {
        if region.closed || self.is_stopped() {
            return Err(RegionError::Unavailable);
        }
        let current = region.current.as_mut().ok_or(RegionError::Unavailable)?;
        if current.sequence != self.sequence || !Arc::ptr_eq(&current.stopped, &self.stopped) {
            return Err(RegionError::Unavailable);
        }
        Ok(current)
    }
    /// Binds a job submitted for this permit. On failure, cancel that exact job outside the lock.
    #[cfg(test)]
    pub fn bind_job(&self, job: &Id) -> Result<(), RegionError> {
        let mut region = self.region.lock().map_err(|_| RegionError::Poisoned)?;
        let current = self.current_mut(&mut region)?;
        if current.finished {
            return Err(RegionError::Unavailable);
        }
        if current.job.is_some() {
            return Err(RegionError::Unavailable);
        }
        current.job = Some(job.clone());
        Ok(())
    }
    /// Runs one short in-memory publication while this request remains current.
    /// The callback must not call F24, wait for OS work, or wait for a worker.
    /// Callers must release the region lock before entering F24's publication gate.
    pub fn publish<T, E>(
        &self,
        synchronous: bool,
        publish: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<T, E>, RegionError> {
        let mut region = self.region.lock().map_err(|_| RegionError::Poisoned)?;
        let current = self.current_mut(&mut region)?;
        if current.finished || current.published {
            return Err(RegionError::Unavailable);
        }
        let result = publish();
        current.published = true;
        if synchronous {
            current.finished = true;
        }
        Ok(result)
    }
}
pub(crate) mod commands;
#[cfg(test)]
mod tests;
