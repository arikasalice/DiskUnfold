//! Session-only registry state: admitted results, their access order and released sources.
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use du_core_contracts::Id;
use du_core_contracts::result::ResultVersionRead;

use super::{MultiFolderKey, QueryResult};
use crate::publication_events::RetiredVersions;

const MAX_RESULTS: usize = 16;
#[derive(Default)]
pub(super) struct State {
    pub(super) entries: HashMap<Id, Entry>,
    pub(super) lru: VecDeque<Id>,
    pub(super) used: HashSet<Id>,
    pub(super) released_sources: HashSet<Id>,
}

pub(super) struct Entry {
    pub(super) result: Arc<QueryResult>,
    pub(super) folder_key: Option<MultiFolderKey>,
}

impl State {
    pub(super) fn folder_id(&self, key: &MultiFolderKey) -> Option<Id> {
        self.entries
            .iter()
            .find_map(|(id, entry)| (entry.folder_key.as_ref() == Some(key)).then(|| id.clone()))
    }

    /// Stores a result and returns it together with the entry the LRU displaced.
    ///
    /// The caller must drop the displaced entry only after releasing the registry lock,
    /// because freeing the last reference to a large result can take long.
    pub(super) fn insert(
        &mut self,
        id: Id,
        result: QueryResult,
        folder_key: Option<MultiFolderKey>,
    ) -> (Arc<QueryResult>, Option<Entry>) {
        let evicted = if self.entries.len() == MAX_RESULTS
            && let Some(oldest) = self.lru.pop_front()
        {
            self.entries.remove(&oldest)
        } else {
            None
        };
        self.used.insert(id.clone());
        self.lru.push_back(id.clone());
        let result = Arc::new(result);
        self.entries.insert(
            id,
            Entry {
                result: Arc::clone(&result),
                folder_key,
            },
        );
        (result, evicted)
    }

    /// Removes the entries of a source version and returns them undropped.
    ///
    /// The caller must drop them only after releasing the registry lock.
    pub(super) fn drop_source(&mut self, version: &Id) -> Vec<Entry> {
        let ids: Vec<Id> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.result.version.version_id() == version)
            .map(|(id, _)| id.clone())
            .collect();
        let removed: Vec<Entry> = ids
            .iter()
            .filter_map(|id| self.entries.remove(id))
            .collect();
        let entries = &self.entries;
        self.lru.retain(|id| entries.contains_key(id));
        removed
    }

    /// Removes one entry and returns it undropped, for the caller to drop outside the lock.
    pub(super) fn remove(&mut self, id: &Id) -> Option<Entry> {
        let removed = self.entries.remove(id);
        self.lru.retain(|candidate| candidate != id);
        removed
    }
}

impl RetiredVersions for Mutex<State> {
    fn retire(&self, version: &Id) {
        let dropped = {
            let Ok(mut state) = self.lock() else {
                return;
            };
            // A retired version never becomes current again, so `pin_current` already
            // rejects it. Recording it only matters when results were dropped; this keeps
            // the released set from growing with every partial publication.
            let dropped = state.drop_source(version);
            if !dropped.is_empty() {
                state.released_sources.insert(version.clone());
            }
            dropped
        };
        // The registry lock is released, so freeing large results blocks nobody.
        drop(dropped);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use super::super::touch;
    use super::*;
    use crate::query::{self, Basis, Condition, Registry, Sort};
    use crate::{IngestSink, Observation};
    use du_core_contracts::result::ResultCompleteness;

    fn id(n: u64) -> Result<Id, String> {
        Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
    }

    /// Builds a sealed version of one file and runs a full query over it.
    fn result(scope: u64, version: u64) -> Result<QueryResult, String> {
        let sink = IngestSink::default();
        let mut build = sink.open(id(scope)?).map_err(|e| format!("{e:?}"))?;
        build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        build
            .add_file(Observation {
                path: vec![b"a".to_vec()],
                size: None,
            })
            .map_err(|e| format!("{e:?}"))?;
        let sealed = build
            .seal(id(version)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let sort = Sort {
            id: "sort.path",
            revision: 1,
            basis: Basis::Logical,
            descending: false,
        };
        query::run(
            &Registry::baseline(),
            sealed,
            &Condition::All(vec![]),
            sort,
            || false,
        )
        .map_err(|e| format!("{e:?}"))
    }

    #[test]
    fn drop_source_returns_the_removed_entries_undropped() -> Result<(), String> {
        let mut state = State::default();
        let (first, _) = state.insert(id(10)?, result(1, 2)?, None);
        let (second, _) = state.insert(id(11)?, result(1, 2)?, None);
        let (other, _) = state.insert(id(12)?, result(3, 4)?, None);
        let source = first.version.version_id().clone();
        let weak: Vec<Weak<QueryResult>> = [&first, &second]
            .iter()
            .map(|result| Arc::downgrade(result))
            .collect();
        drop((first, second));

        let removed = state.drop_source(&source);
        assert_eq!(removed.len(), 2);
        assert!(weak.iter().all(|result| result.upgrade().is_some()));
        assert_eq!(state.entries.len(), 1);
        assert!(state.entries.contains_key(&id(12)?));
        assert_eq!(state.lru, VecDeque::from([id(12)?]));
        assert!(state.drop_source(&source).is_empty());
        drop((removed, other));
        assert!(weak.iter().all(|result| result.upgrade().is_none()));
        Ok(())
    }

    #[test]
    fn insert_returns_the_displaced_entry_in_lru_order() -> Result<(), String> {
        let mut state = State::default();
        for n in 0..MAX_RESULTS as u64 {
            let (_, evicted) = state.insert(id(100 + n)?, result(1, 2)?, None);
            assert!(evicted.is_none());
        }
        touch(&mut state.lru, &id(100)?);
        let (kept, evicted) = state.insert(id(200)?, result(1, 2)?, None);
        let evicted = evicted.ok_or("the least recently used entry is displaced")?;
        let weak = Arc::downgrade(&evicted.result);
        assert!(!state.entries.contains_key(&id(101)?));
        assert!(state.entries.contains_key(&id(100)?));
        assert_eq!(state.entries.len(), MAX_RESULTS);
        assert_eq!(state.lru.front(), Some(&id(102)?));
        assert_eq!(state.lru.back(), Some(&id(200)?));
        assert!(
            weak.upgrade().is_some(),
            "the caller owns the displaced result"
        );
        drop((evicted, kept));
        assert!(weak.upgrade().is_none());
        Ok(())
    }
}
