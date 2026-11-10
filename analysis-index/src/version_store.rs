//! Session result lineages and current-version ownership.
use crate::{BuildError, IngestSink, QueryError, ResultVersion};
use du_core_contracts::{Id, analysis_ipc::ScanResult};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

type Lineage = Option<Id>;

#[derive(Clone)]
pub(super) struct Entry {
    pub(super) result: Arc<ResultVersion>,
    order: u64,
}

#[derive(Default)]
pub(super) struct State {
    lineages: HashMap<String, Id>,
    current: HashMap<Lineage, Entry>,
    next_order: u64,
    event_sequences: HashMap<Id, u64>,
    active: HashSet<Lineage>,
    used_versions: HashSet<Id>,
}
impl State {
    pub(super) fn next_event_sequence(&mut self, lineage: Option<&Id>) -> Option<u64> {
        let sequence = self.event_sequences.entry(lineage?.clone()).or_default();
        let next = sequence
            .checked_add(1)
            .filter(|next| *next < (1_u64 << 53))?;
        *sequence = next;
        Some(next)
    }

    pub(super) fn bind(&mut self, fingerprint: &str, candidate: Id) -> Result<Id, BuildError> {
        if let Some(id) = self.lineages.get(fingerprint) {
            return Ok(id.clone());
        }
        if self.lineages.values().any(|id| id == &candidate) {
            return Err(BuildError::DuplicateVersion);
        }
        self.lineages
            .insert(fingerprint.to_owned(), candidate.clone());
        Ok(candidate)
    }
    pub(super) fn pin(&self) -> Option<Arc<ResultVersion>> {
        self.current
            .values()
            .max_by_key(|entry| entry.order)
            .map(|entry| Arc::clone(&entry.result))
    }
    pub(super) fn pin_current(&self, expected: &Id) -> Result<Arc<ResultVersion>, QueryError> {
        if let Some(result) = self
            .current
            .values()
            .find(|value| value.result.version_id == *expected)
        {
            return Ok(Arc::clone(&result.result));
        }
        if self.current.is_empty() {
            Err(QueryError::NotFound)
        } else {
            Err(QueryError::StaleVersion)
        }
    }
    pub(super) fn begin(&mut self, lineage: &Lineage) -> Result<Option<Entry>, BuildError> {
        if lineage
            .as_ref()
            .is_some_and(|id| !self.lineages.values().any(|bound| bound == id))
        {
            return Err(BuildError::Unavailable);
        }
        if !self.active.insert(lineage.clone()) {
            return Err(BuildError::Busy);
        }
        Ok(self.current.get(lineage).cloned())
    }
    pub(super) fn publish(
        &mut self,
        lineage: &Lineage,
        result: Arc<ResultVersion>,
        finish: bool,
    ) -> Result<Option<Entry>, BuildError> {
        let order = self
            .next_order
            .checked_add(1)
            .ok_or(BuildError::CapacityExceeded)?;
        if !self.used_versions.insert(result.version_id.clone()) {
            return Err(BuildError::DuplicateVersion);
        }
        self.next_order = order;
        let previous = self
            .current
            .insert(lineage.clone(), Entry { result, order });
        if finish {
            self.active.remove(lineage);
        }
        Ok(previous)
    }
    pub(super) fn abort(
        &mut self,
        lineage: &Lineage,
        baseline: Option<Entry>,
        published: bool,
    ) -> Option<Entry> {
        self.active.remove(lineage);
        if !published {
            return None;
        }
        if let Some(baseline) = baseline {
            self.current.insert(lineage.clone(), baseline)
        } else {
            self.current.remove(lineage)
        }
    }
}

impl IngestSink {
    /// Reads a lineage sequence and this scope version's current result atomically.
    pub fn scan_result(
        &self,
        lineage_id: &Id,
        scope_version: &Id,
    ) -> Result<ScanResult, BuildError> {
        let state = self.state.lock().map_err(|_| BuildError::Unavailable)?;
        let sequence = state.event_sequences.get(lineage_id).copied().unwrap_or(0);
        let result_version = state
            .current
            .get(&Some(lineage_id.clone()))
            .filter(|entry| entry.result.scope_version_id == *scope_version)
            .map(|entry| entry.result.version_id.clone());
        Ok(ScanResult {
            lineage_id: lineage_id.clone(),
            sequence,
            result_version,
        })
    }
}
