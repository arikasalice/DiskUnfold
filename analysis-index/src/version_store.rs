//! Session result lineages and current-version ownership.
use crate::{BuildError, IngestSink, QueryError, ResultOrigin, ResultVersion, ScanCompletion};
use du_core_contracts::{Id, analysis_ipc::ScanResult, result::ResultCompleteness};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock, Weak},
};

type Lineage = Option<Id>;

#[derive(Clone)]
pub(super) struct Entry {
    pub(super) result: Arc<ResultVersion>,
    order: u64,
    pub(super) completion: Option<Arc<OnceLock<ScanCompletion>>>,
}

#[derive(Default)]
pub(super) struct State {
    lineages: HashMap<String, Id>,
    current: HashMap<Lineage, Entry>,
    next_order: u64,
    event_sequences: HashMap<Id, u64>,
    active: HashSet<Lineage>,
    used_versions: HashSet<Id>,
    pub(super) awaiting_terminal: HashMap<Id, Weak<OnceLock<ScanCompletion>>>,
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
        self.current_entry(expected)
            .map(|entry| Arc::clone(&entry.result))
    }

    pub(super) fn current_entry(&self, expected: &Id) -> Result<&Entry, QueryError> {
        if let Some(entry) = self
            .current
            .values()
            .find(|value| value.result.version_id == *expected)
        {
            return Ok(entry);
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
    pub(super) fn begin_replacement(
        &self,
        lineage: &Lineage,
        expected_base: &Id,
    ) -> Result<Entry, BuildError> {
        // An active scan owns rollback of this lineage's current version. A
        // replacement must not publish while that scan can still abort and
        // restore its older baseline over the replacement.
        if self.active.contains(lineage) {
            return Err(BuildError::Busy);
        }
        let entry = self
            .current
            .get(lineage)
            .ok_or(BuildError::BaseSuperseded)?;
        if entry.result.version_id != *expected_base {
            return Err(BuildError::BaseSuperseded);
        }
        Ok(entry.clone())
    }
    pub(super) fn publish(
        &mut self,
        lineage: &Lineage,
        result: Arc<ResultVersion>,
        finish: bool,
        expected_base: Option<&Id>,
        reserved: bool,
    ) -> Result<Option<Entry>, BuildError> {
        if expected_base.is_some() && self.active.contains(lineage) {
            return Err(BuildError::Busy);
        }
        if expected_base.is_some_and(|expected| {
            self.current
                .get(lineage)
                .is_none_or(|entry| entry.result.version_id != *expected)
        }) {
            return Err(BuildError::BaseSuperseded);
        }
        let order = self
            .next_order
            .checked_add(1)
            .ok_or(BuildError::CapacityExceeded)?;
        if !self.used_versions.insert(result.version_id.clone()) {
            return Err(BuildError::DuplicateVersion);
        }
        self.next_order = order;
        let completion = self.await_terminal(&result, finish);
        let previous = self.current.insert(
            lineage.clone(),
            Entry {
                result,
                order,
                completion,
            },
        );
        if finish && reserved {
            self.active.remove(lineage);
        }
        Ok(previous)
    }

    fn await_terminal(
        &mut self,
        result: &ResultVersion,
        finish: bool,
    ) -> Option<Arc<OnceLock<ScanCompletion>>> {
        if !finish
            || result.origin != ResultOrigin::Scan
            || result.lineage_id.is_none()
            || !matches!(
                result.completeness,
                ResultCompleteness::Complete | ResultCompleteness::Partial
            )
        {
            return None;
        }
        self.awaiting_terminal
            .retain(|_, proof| proof.strong_count() != 0);
        let proof = Arc::new(OnceLock::new());
        self.awaiting_terminal
            .insert(result.version_id.clone(), Arc::downgrade(&proof));
        Some(proof)
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
    /// Pins an exact current scan revision, or the latest current scan at admission.
    /// An optional lineage limits the lookup; anonymous builds are never scan sources.
    pub fn pin_scan(
        &self,
        lineage: Option<&Id>,
        revision: Option<&Id>,
    ) -> Result<Arc<ResultVersion>, QueryError> {
        let state = self.state.lock().map_err(|_| QueryError::Unavailable)?;
        if let Some(lineage) = lineage {
            let entry = state
                .current
                .get(&Some(lineage.clone()))
                .ok_or(QueryError::NotFound)?;
            if revision.is_some_and(|expected| entry.result.version_id != *expected) {
                return Err(QueryError::StaleVersion);
            }
            return Ok(Arc::clone(&entry.result));
        }
        if let Some(revision) = revision {
            let result = state.pin_current(revision)?;
            return result
                .lineage_id
                .is_some()
                .then_some(result)
                .ok_or(QueryError::NotFound);
        }
        state
            .current
            .values()
            .filter(|entry| entry.result.lineage_id.is_some())
            .max_by_key(|entry| entry.order)
            .map(|entry| Arc::clone(&entry.result))
            .ok_or(QueryError::NotFound)
    }

    /// Runs a bounded in-memory operation while the exact result remains current.
    ///
    /// The callback runs while the result-store state lock is held. It must not
    /// reenter this store, perform I/O, wait on channels or tasks, or call
    /// runtime or application callbacks. If it takes another lock, keep the
    /// order acyclic and document it at the call site. The pinned result
    /// identifies its lineage, so admission does not scan other lineages. The
    /// map lookup is not a worst-case constant-time guarantee.
    pub fn with_current<T>(
        &self,
        expected: &Arc<ResultVersion>,
        operation: impl FnOnce() -> T,
    ) -> Result<T, QueryError> {
        let state = self.state.lock().map_err(|_| QueryError::Unavailable)?;
        let entry = state.current.get(&expected.lineage_id).ok_or_else(|| {
            if state.current.is_empty() {
                QueryError::NotFound
            } else {
                QueryError::StaleVersion
            }
        })?;
        if !Arc::ptr_eq(&entry.result, expected) {
            return Err(QueryError::StaleVersion);
        }
        Ok(operation())
    }

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
