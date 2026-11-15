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

    pub(super) fn insert(
        &mut self,
        id: Id,
        result: QueryResult,
        folder_key: Option<MultiFolderKey>,
    ) -> Arc<QueryResult> {
        if self.entries.len() == MAX_RESULTS
            && let Some(oldest) = self.lru.pop_front()
        {
            self.entries.remove(&oldest);
        }
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
        result
    }

    /// Drops the results of a source version; returns whether any was dropped.
    pub(super) fn drop_source(&mut self, version: &Id) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| entry.result.version.version_id() != version);
        let entries = &self.entries;
        self.lru.retain(|id| entries.contains_key(id));
        self.entries.len() != before
    }
}

impl RetiredVersions for Mutex<State> {
    fn retire(&self, version: &Id) {
        if let Ok(mut state) = self.lock() {
            // A retired version never becomes current again, so `pin_current` already
            // rejects it. Recording it only matters when results were dropped; this keeps
            // the released set from growing with every partial publication.
            if state.drop_source(version) {
                state.released_sources.insert(version.clone());
            }
        }
    }
}
