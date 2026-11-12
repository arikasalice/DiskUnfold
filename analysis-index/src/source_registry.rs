//! Session-local identities and accepted sequence watermarks for freshness writers.
//!
//! Sequence inspection is read-only. Writer closures run while source admission is
//! serialized, and failed writes leave the accepted sequence unchanged.

use du_core_contracts::Id;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

/// The kind of IF-17 producer represented by a registered source lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// A watch source whose first accepted sequence follows an IF-11 snapshot cutoff.
    Watch,
    /// A direct refresh source, numbered from one for each new lifetime.
    Direct,
    /// A snapshot-application source, numbered from one for each new lifetime.
    Snapshot,
}

/// An opaque, non-cloneable capability for one registration lifetime.
#[derive(Debug)]
pub struct SourceHandle {
    source_id: Id,
    generation: NonZeroU64,
    registry_identity: Arc<()>,
    kind: SourceKind,
}

impl SourceHandle {
    /// Returns the registered source identifier without exposing lifetime state.
    pub fn source_id(&self) -> &Id {
        &self.source_id
    }

    /// Returns the source kind fixed at registration.
    pub fn kind(&self) -> SourceKind {
        self.kind
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceState {
    generation: NonZeroU64,
    accepted_sequence: u64,
    ended: bool,
}

/// A source identity was reused or an operation did not match its live lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// The source identifier has already been registered, including ended sources.
    DuplicateSource,
    /// The supplied identifier and lifetime handle do not identify the same source.
    CrossSource,
    /// The source lifetime has ended or its handle is stale.
    Expired,
    /// The caller's candidate is not the next accepted sequence.
    SequenceMismatch,
    /// Computing the next sequence would overflow `u64`.
    SequenceOverflow,
    /// Watch sources must be registered through `register_watch`.
    WatchRequiresSnapshot,
    /// The internal lifetime generation space is exhausted.
    GenerationOverflow,
    /// The registry mutex was poisoned.
    Unavailable,
}

/// Failure to accept the IF-11 snapshot used to establish a watch source cutoff.
#[derive(Debug, PartialEq, Eq)]
pub enum WatchRegistrationError<E> {
    /// The source ID or registry could not be admitted.
    Registry(RegistryError),
    /// The IF-11 snapshot writer failed; no source lifetime was registered.
    Snapshot(E),
}

/// Failure to validate a candidate or commit its atomic freshness write.
#[derive(Debug, PartialEq, Eq)]
pub enum SequenceApplyError<E> {
    /// The source lifetime or candidate sequence was not admissible.
    Registry(RegistryError),
    /// The writer rejected the change; the accepted sequence remains unchanged.
    Writer(E),
}

/// Owns source lifetimes and the last sequence committed by the freshness writer.
#[derive(Default)]
pub struct SourceRegistry {
    state: Mutex<RegistryState>,
}

#[derive(Default)]
struct RegistryState {
    next_generation: u64,
    sources: HashMap<Id, SourceState>,
    identity: Arc<()>,
}

impl SourceRegistry {
    /// Registers a direct or snapshot source exactly once with accepted sequence zero.
    pub fn register(&self, source_id: Id, kind: SourceKind) -> Result<SourceHandle, RegistryError> {
        if kind == SourceKind::Watch {
            return Err(RegistryError::WatchRequiresSnapshot);
        }
        self.register_at(source_id, kind, 0)
    }

    /// Accepts an IF-11 snapshot and registers its watch source at the snapshot cutoff.
    ///
    /// The snapshot writer runs before registration is committed and while source
    /// admission is serialized. If it fails, the source remains unregistered.
    pub fn register_watch<E>(
        &self,
        source_id: Id,
        apply_snapshot: impl FnOnce() -> Result<u64, E>,
    ) -> Result<SourceHandle, WatchRegistrationError<E>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| WatchRegistrationError::Registry(RegistryError::Unavailable))?;
        if state.sources.contains_key(&source_id) {
            return Err(WatchRegistrationError::Registry(
                RegistryError::DuplicateSource,
            ));
        }
        let generation = state
            .next_generation
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(WatchRegistrationError::Registry(
                RegistryError::GenerationOverflow,
            ))?;
        let accepted_sequence = apply_snapshot().map_err(WatchRegistrationError::Snapshot)?;
        Ok(Self::insert_source(
            &mut state,
            source_id,
            generation,
            SourceKind::Watch,
            accepted_sequence,
        ))
    }

    fn register_at(
        &self,
        source_id: Id,
        kind: SourceKind,
        accepted_sequence: u64,
    ) -> Result<SourceHandle, RegistryError> {
        let mut state = self.state.lock().map_err(|_| RegistryError::Unavailable)?;
        if state.sources.contains_key(&source_id) {
            return Err(RegistryError::DuplicateSource);
        }
        let generation = state
            .next_generation
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(RegistryError::GenerationOverflow)?;
        Ok(Self::insert_source(
            &mut state,
            source_id,
            generation,
            kind,
            accepted_sequence,
        ))
    }

    fn insert_source(
        state: &mut RegistryState,
        source_id: Id,
        generation: NonZeroU64,
        kind: SourceKind,
        accepted_sequence: u64,
    ) -> SourceHandle {
        state.next_generation = generation.get();
        state.sources.insert(
            source_id.clone(),
            SourceState {
                generation,
                accepted_sequence,
                ended: false,
            },
        );
        SourceHandle {
            source_id,
            generation,
            registry_identity: Arc::clone(&state.identity),
            kind,
        }
    }

    /// Returns the next candidate sequence without changing registry state.
    pub fn next_sequence(
        &self,
        source_id: &Id,
        handle: &SourceHandle,
    ) -> Result<u64, RegistryError> {
        let state = self.state.lock().map_err(|_| RegistryError::Unavailable)?;
        let source = Self::live_source(&state, source_id, handle)?;
        source
            .accepted_sequence
            .checked_add(1)
            .ok_or(RegistryError::SequenceOverflow)
    }

    /// Runs the atomic writer for the exact next sequence and records it on success.
    ///
    /// Candidate validation, the supplied atomic writer, and watermark advancement
    /// run under one source-registry lock. A rejected candidate never invokes the
    /// writer. A failed writer leaves the accepted sequence unchanged.
    pub fn apply_next<T, E>(
        &self,
        source_id: &Id,
        handle: &SourceHandle,
        sequence: u64,
        writer: impl FnOnce(u64) -> Result<T, E>,
    ) -> Result<T, SequenceApplyError<E>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SequenceApplyError::Registry(RegistryError::Unavailable))?;
        let source = Self::live_source_mut(&mut state, source_id, handle)
            .map_err(SequenceApplyError::Registry)?;
        let expected =
            source
                .accepted_sequence
                .checked_add(1)
                .ok_or(SequenceApplyError::Registry(
                    RegistryError::SequenceOverflow,
                ))?;
        if sequence != expected {
            return Err(SequenceApplyError::Registry(
                RegistryError::SequenceMismatch,
            ));
        }
        let result = writer(sequence).map_err(SequenceApplyError::Writer)?;
        source.accepted_sequence = sequence;
        Ok(result)
    }

    /// Ends a lifetime permanently; the source identifier cannot be registered again.
    pub fn end(&self, source_id: &Id, handle: &SourceHandle) -> Result<(), RegistryError> {
        let mut state = self.state.lock().map_err(|_| RegistryError::Unavailable)?;
        let source = Self::live_source_mut(&mut state, source_id, handle)?;
        source.ended = true;
        Ok(())
    }

    fn live_source<'a>(
        state: &'a RegistryState,
        source_id: &Id,
        handle: &SourceHandle,
    ) -> Result<&'a SourceState, RegistryError> {
        let source = Self::matching_source(state, source_id, handle)?;
        if source.ended {
            return Err(RegistryError::Expired);
        }
        Ok(source)
    }

    fn live_source_mut<'a>(
        state: &'a mut RegistryState,
        source_id: &Id,
        handle: &SourceHandle,
    ) -> Result<&'a mut SourceState, RegistryError> {
        let source = Self::matching_source_mut(state, source_id, handle)?;
        if source.ended {
            return Err(RegistryError::Expired);
        }
        Ok(source)
    }

    fn matching_source<'a>(
        state: &'a RegistryState,
        source_id: &Id,
        handle: &SourceHandle,
    ) -> Result<&'a SourceState, RegistryError> {
        if !Arc::ptr_eq(&state.identity, &handle.registry_identity) {
            return Err(RegistryError::CrossSource);
        }
        if source_id != &handle.source_id {
            return Err(RegistryError::CrossSource);
        }
        let source = state.sources.get(source_id).ok_or(RegistryError::Expired)?;
        if source.generation != handle.generation {
            return Err(RegistryError::Expired);
        }
        Ok(source)
    }

    fn matching_source_mut<'a>(
        state: &'a mut RegistryState,
        source_id: &Id,
        handle: &SourceHandle,
    ) -> Result<&'a mut SourceState, RegistryError> {
        if !Arc::ptr_eq(&state.identity, &handle.registry_identity) {
            return Err(RegistryError::CrossSource);
        }
        if source_id != &handle.source_id {
            return Err(RegistryError::CrossSource);
        }
        let source = state
            .sources
            .get_mut(source_id)
            .ok_or(RegistryError::Expired)?;
        if source.generation != handle.generation {
            return Err(RegistryError::Expired);
        }
        Ok(source)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
