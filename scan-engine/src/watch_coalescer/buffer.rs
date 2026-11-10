use super::model::{Change, ChangeKind, Config, Key, Signal};
use std::collections::{BTreeMap, BTreeSet};

/// A journal holds only keys touched by one signal; it permits lossless retry.
#[derive(Default)]
pub(super) struct Undo {
    originals: BTreeMap<Key, Option<Change>>,
    opened: Option<u64>,
    last: Option<u64>,
}
#[derive(Clone, Default)]
pub(super) struct Buffer {
    entries: BTreeMap<Key, Change>,
    /// Number of immediate keys under each parent, updated with the entries.
    parents: BTreeMap<Key, usize>,
    pub opened: Option<u64>,
    pub last: Option<u64>,
    #[cfg(test)]
    work: usize,
}
impl Buffer {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    // All input timestamps are monotonic, so the first input is earliest even
    // after its key is absorbed or promoted.
    pub fn first(&self) -> Option<u64> {
        self.opened
    }
    pub fn due(&self, config: Config, at: u64) -> bool {
        self.opened
            .is_some_and(|start| at.saturating_sub(start) >= config.window_ms)
    }
    #[cfg(test)]
    pub fn last_work(&self) -> usize {
        self.work
    }
    pub fn push(&mut self, signal: &Signal, config: Config) -> Undo {
        #[cfg(test)]
        {
            self.work = 0;
        }
        let mut undo = Undo {
            originals: BTreeMap::new(),
            opened: self.opened,
            last: self.last,
        };
        self.opened.get_or_insert(signal.observed_at);
        self.last = Some(signal.observed_at);
        if let Some(path) = &signal.path {
            let mut parent = path.clone();
            parent.pop();
            self.insert(
                change(parent, false, signal.kind, signal.observed_at),
                config,
                &mut undo,
            );
            if signal.directory_subtree {
                self.insert(
                    change(path.clone(), true, signal.kind, signal.observed_at),
                    config,
                    &mut undo,
                );
            }
        } else {
            self.insert(
                change(Vec::new(), true, ChangeKind::Unknown, signal.observed_at),
                config,
                &mut undo,
            );
        }
        undo
    }
    fn insert(&mut self, mut value: Change, config: Config, undo: &mut Undo) {
        for depth in 0..value.path.len() {
            #[cfg(test)]
            {
                self.work += 1;
            }
            let ancestor = &value.path[..depth];
            if self.entries.get(ancestor).is_some_and(|item| item.subtree) {
                self.record(ancestor, undo);
                if let Some(parent) = self.entries.get_mut(ancestor) {
                    merge(parent, &value);
                }
                return;
            }
        }
        if let Some(same) = self.remove(&value.path, undo) {
            value.subtree |= same.subtree;
            merge(&mut value, &same);
        }
        if value.subtree {
            // Keys with a component-vector prefix form a contiguous BTreeMap range.
            let descendants: Vec<_> = self
                .entries
                .range(value.path.clone()..)
                .take_while(|(key, _)| key.starts_with(&value.path))
                .map(|(key, _)| key.clone())
                .collect();
            #[cfg(test)]
            {
                self.work += descendants.len();
            }
            for key in descendants {
                if let Some(item) = self.remove(&key, undo) {
                    merge(&mut value, &item);
                }
            }
        }
        let key = value.path.clone();
        self.put(value, undo);
        let Some(parent) = key
            .get(..key.len().saturating_sub(1))
            .filter(|_| !key.is_empty())
        else {
            return;
        };
        if self.parents.get(parent).copied().unwrap_or(0) >= config.promote_at {
            let mut promoted = change(parent.to_vec(), true, ChangeKind::Unknown, u64::MAX);
            promoted.last = 0;
            promoted.kinds.clear();
            self.insert(promoted, config, undo);
        }
    }
    fn record(&self, key: &[Vec<u8>], undo: &mut Undo) {
        if !undo.originals.contains_key(key) {
            undo.originals
                .insert(key.to_vec(), self.entries.get(key).cloned());
        }
    }
    fn add_parent(&mut self, key: &[Vec<u8>]) {
        if let Some(parent) = key
            .get(..key.len().saturating_sub(1))
            .filter(|_| !key.is_empty())
        {
            *self.parents.entry(parent.to_vec()).or_insert(0) += 1;
        }
    }
    fn remove_parent(&mut self, key: &[Vec<u8>]) {
        if let Some(parent) = key
            .get(..key.len().saturating_sub(1))
            .filter(|_| !key.is_empty())
            && let Some(count) = self.parents.get_mut(parent)
        {
            *count -= 1;
            if *count == 0 {
                self.parents.remove(parent);
            }
        }
    }
    fn remove(&mut self, key: &[Vec<u8>], undo: &mut Undo) -> Option<Change> {
        if self.entries.contains_key(key) {
            self.record(key, undo);
        }
        let removed = self.entries.remove(key);
        if removed.is_some() {
            self.remove_parent(key);
        }
        removed
    }
    fn put(&mut self, value: Change, undo: &mut Undo) {
        self.record(&value.path, undo);
        self.add_parent(&value.path);
        self.entries.insert(value.path.clone(), value);
    }
    pub fn rollback(&mut self, undo: Undo) {
        for (key, original) in undo.originals {
            if self.entries.remove(&key).is_some() {
                self.remove_parent(&key);
            }
            if let Some(value) = original {
                self.add_parent(&key);
                self.entries.insert(key, value);
            }
        }
        self.opened = undo.opened;
        self.last = undo.last;
    }
    pub fn take(&mut self) -> Vec<Change> {
        self.opened = None;
        self.last = None;
        self.parents.clear();
        std::mem::take(&mut self.entries).into_values().collect()
    }
}
fn change(path: Key, subtree: bool, kind: ChangeKind, at: u64) -> Change {
    Change {
        path,
        subtree,
        kinds: BTreeSet::from([kind]),
        first: at,
        last: at,
    }
}
fn merge(into: &mut Change, item: &Change) {
    into.kinds.extend(&item.kinds);
    into.first = into.first.min(item.first);
    into.last = into.last.max(item.last);
}
