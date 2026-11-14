//! Session-only store of registered classifications.

use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

use du_core_contracts::Id;

use super::{Classification, Error};

/// A store keeps this many classifications unless a capacity is given.
const DEFAULT_CAPACITY: usize = 8;

/// Registered classifications by reference, evicting the least recently used when full.
///
/// A registered classification is shared and never changes. The store lives in session
/// memory only. Its lock is never held while a caller's callback or an evicted value runs.
pub struct ClassificationStore {
    inner: Mutex<Entries>,
}

/// Entries from least to most recently used.
struct Entries {
    capacity: NonZeroUsize,
    entries: Vec<(Id, Arc<Classification>)>,
}

impl Default for ClassificationStore {
    /// A store with room for eight classifications.
    fn default() -> Self {
        Self::with_capacity(NonZeroUsize::new(DEFAULT_CAPACITY).unwrap_or(NonZeroUsize::MIN))
    }
}

impl ClassificationStore {
    /// Creates an empty store that keeps at most `capacity` classifications.
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        Self {
            inner: Mutex::new(Entries {
                capacity,
                entries: Vec::new(),
            }),
        }
    }

    /// Registers `classification` as the most recently used entry under `id`.
    ///
    /// The partition of the scope is checked first: a classification whose counts do not
    /// reconcile is rejected and nothing is stored. An existing entry with the same `id` is
    /// replaced, and the least recently used entries beyond the capacity are removed.
    ///
    /// # Errors
    /// Returns [`Error::InconsistentMembership`] when the counts do not reconcile and
    /// [`Error::Unavailable`] when the store lock is poisoned.
    pub fn register(&self, id: Id, classification: Arc<Classification>) -> Result<(), Error> {
        classification.verify()?;
        let mut inner = self.inner.lock().map_err(|_| Error::Unavailable)?;
        inner.entries.retain(|(existing, _)| existing != &id);
        inner.entries.push((id, classification));
        let excess = inner.entries.len().saturating_sub(inner.capacity.get());
        let evicted: Vec<_> = inner.entries.drain(..excess).collect();
        drop(inner);
        drop(evicted);
        Ok(())
    }

    /// Returns the classification registered under `id` and marks it most recently used.
    /// A poisoned store holds nothing that can be returned.
    pub fn get(&self, id: &Id) -> Option<Arc<Classification>> {
        let mut inner = self.inner.lock().ok()?;
        let position = inner.entries.iter().position(|(held, _)| held == id)?;
        let entry = inner.entries.remove(position);
        let found = Arc::clone(&entry.1);
        inner.entries.push(entry);
        Some(found)
    }

    /// Removes every classification for which `keep` returns false, for results whose
    /// source version is no longer available. `keep` runs without the store lock held.
    pub fn retain(&self, mut keep: impl FnMut(&Classification) -> bool) {
        let Ok(inner) = self.inner.lock() else {
            return;
        };
        let held: Vec<_> = inner
            .entries
            .iter()
            .map(|(_, classification)| Arc::clone(classification))
            .collect();
        drop(inner);
        let removed: Vec<_> = held.into_iter().filter(|item| !keep(item)).collect();
        if removed.is_empty() {
            return;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner
            .entries
            .retain(|(_, held)| !removed.iter().any(|gone| Arc::ptr_eq(gone, held)));
    }
}

#[cfg(test)]
mod tests;
