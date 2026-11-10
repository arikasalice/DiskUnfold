//! Session-only immutable query results and admitted, pinned page reads.
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::sync::{Arc, Mutex};

use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};
use du_core_contracts::{Id, ItemRef, Timestamp};

use super::{Basis, Condition, QueryResult};
use crate::{IngestSink, QueryError};

/// Default page size and maximum accepted page size.
pub const DEFAULT_PAGE_SIZE: u32 = 200;
pub const MAX_PAGE_SIZE: u32 = 1_000;
const MAX_RESULTS: usize = 16;

/// Session result or page admission failure; transport error mapping is a caller concern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageError {
    StaleVersion,
    InvalidRange,
    DuplicateId,
    InvalidResult,
    Unavailable,
}

#[derive(Default)]
struct State {
    entries: HashMap<Id, Arc<QueryResult>>,
    lru: VecDeque<Id>,
    used: HashSet<Id>,
    released_sources: HashSet<Id>,
}

/// In-memory result registry bound to one live source. Dropping it releases its entries.
///
/// A caller releasing a source version must also invoke [`Self::release_source`].
pub struct ResultSets {
    source: IngestSink,
    state: Mutex<State>,
}

impl ResultSets {
    /// Creates an empty session registry for the supplied source publication point.
    pub fn new(source: IngestSink) -> Self {
        Self {
            source,
            state: Mutex::new(State::default()),
        }
    }

    /// Atomically registers one completed result under a caller-provided fresh identifier.
    ///
    /// The source must still be current at admission; duplicate IDs are never reused in
    /// this registry. A cancelled evaluation must not be supplied for registration.
    pub fn register(&self, id: Id, result: QueryResult) -> Result<(), PageError> {
        validate_result(&result)?;
        let mut state = self.state.lock().map_err(|_| PageError::Unavailable)?;
        if state.used.contains(&id) {
            return Err(PageError::DuplicateId);
        }
        if state.released_sources.contains(result.version.version_id()) {
            return Err(PageError::StaleVersion);
        }
        let current = self
            .source
            .pin_current(result.version.version_id())
            .map_err(source_error)?;
        if !Arc::ptr_eq(&current, &result.version) {
            return Err(PageError::StaleVersion);
        }
        if state.entries.len() == MAX_RESULTS
            && let Some(oldest) = state.lru.pop_front()
        {
            state.entries.remove(&oldest);
        }
        state.used.insert(id.clone());
        state.lru.push_back(id.clone());
        state.entries.insert(id, Arc::new(result));
        Ok(())
    }

    /// Admits one page against the current source, then pins its immutable contents.
    ///
    /// A page already admitted remains readable after source replacement or release;
    /// each subsequent page must be admitted again and can fail as stale.
    pub fn open_page(
        &self,
        id: &Id,
        version: &Id,
        start: u64,
        count: Option<u32>,
    ) -> Result<Page, PageError> {
        let count = count.unwrap_or(DEFAULT_PAGE_SIZE);
        if count == 0 || count > MAX_PAGE_SIZE {
            return Err(PageError::InvalidRange);
        }
        let end = start
            .checked_add(u64::from(count))
            .ok_or(PageError::InvalidRange)?;
        let mut state = self.state.lock().map_err(|_| PageError::Unavailable)?;
        let result = self.pin_entry(&state, id, version)?;
        let start = usize::try_from(start).map_err(|_| PageError::InvalidRange)?;
        if start > result.items.len() {
            return Err(PageError::InvalidRange);
        }
        let end = usize::try_from(end)
            .unwrap_or(usize::MAX)
            .min(result.items.len());
        touch(&mut state.lru, id);
        Ok(Page {
            result,
            range: start..end,
        })
    }

    /// Admits a parent result for a refinement, using the same expiry rules as page reads.
    pub fn open_parent(&self, id: &Id, version: &Id) -> Result<Arc<QueryResult>, PageError> {
        let mut state = self.state.lock().map_err(|_| PageError::Unavailable)?;
        let result = self.pin_entry(&state, id, version)?;
        touch(&mut state.lru, id);
        Ok(result)
    }

    fn pin_entry(
        &self,
        state: &State,
        id: &Id,
        version: &Id,
    ) -> Result<Arc<QueryResult>, PageError> {
        let result = state.entries.get(id).ok_or(PageError::StaleVersion)?;
        if result.version.version_id() != version {
            return Err(PageError::StaleVersion);
        }
        let current = self.source.pin_current(version).map_err(source_error)?;
        if !Arc::ptr_eq(&current, &result.version) {
            return Err(PageError::StaleVersion);
        }
        Ok(Arc::clone(result))
    }

    /// Withdraws all registered results derived from a released source version.
    pub fn release_source(&self, version: &Id) -> Result<(), PageError> {
        let mut state = self.state.lock().map_err(|_| PageError::Unavailable)?;
        state.released_sources.insert(version.clone());
        state
            .entries
            .retain(|_, value| value.version.version_id() != version);
        let valid: HashSet<Id> = state.entries.keys().cloned().collect();
        state.lru.retain(|id| valid.contains(id));
        Ok(())
    }

    /// Releases one registered result while allowing admitted pages to finish.
    pub fn release(&self, id: &Id) -> Result<(), PageError> {
        let mut state = self.state.lock().map_err(|_| PageError::Unavailable)?;
        state.entries.remove(id);
        state.lru.retain(|candidate| candidate != id);
        Ok(())
    }
}

fn source_error(error: QueryError) -> PageError {
    match error {
        QueryError::Unavailable => PageError::Unavailable,
        _ => PageError::StaleVersion,
    }
}

fn validate_result(result: &QueryResult) -> Result<(), PageError> {
    let mut seen = HashSet::new();
    for item in &result.items {
        let row = result.version.items().get(item.item_no as usize);
        if &item.result_version != result.version.version_id()
            || row.is_none()
            || result
                .folder
                .is_some_and(|parent| row.is_some_and(|row| row.parent != Some(parent)))
            || !seen.insert(item.item_no)
        {
            return Err(PageError::InvalidResult);
        }
    }
    Ok(())
}

fn touch(lru: &mut VecDeque<Id>, id: &Id) {
    if let Some(index) = lru.iter().position(|candidate| candidate == id) {
        lru.remove(index);
    }
    lru.push_back(id.clone());
}

/// Whether the number of matches is final or a lower bound due to incomplete evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Total {
    Exact(u64),
    LowerBound(u64),
}

/// One immutable admitted page; its backing result stays alive until this value is dropped.
pub struct Page {
    result: Arc<QueryResult>,
    range: Range<usize>,
}

impl Page {
    /// Returns the source pinned when this page was admitted.
    pub fn version(&self) -> &Arc<crate::ResultVersion> {
        &self.result.version
    }
    /// Returns the ordered item references in this page.
    pub fn items(&self) -> &[ItemRef] {
        &self.result.items[self.range.clone()]
    }
    /// Returns the position immediately after this page.
    pub fn next_position(&self) -> usize {
        self.range.end
    }
    /// Returns metadata from the same source and evaluation as the page.
    pub fn metadata(&self) -> Metadata<'_> {
        let count = self.result.items.len() as u64;
        // A missing byte count in a known child is not an enumeration gap.
        // Folder-level enumeration evidence is not retained by the current source.
        let completeness = match self.result.folder {
            Some(_) => self.result.version.completeness(),
            None => self.result.version.space(0).map_or_else(
                || self.result.version.completeness(),
                |aggregate| aggregate.completeness,
            ),
        };
        let total = if completeness == ResultCompleteness::Complete
            && self.result.unknown_count == 0
            && !self.result.inherited_uncertainty
        {
            Total::Exact(count)
        } else {
            Total::LowerBound(count)
        };
        Metadata {
            version: self.result.version.version_id(),
            observed_at: self.result.version.observed_at(),
            folder: self.result.folder,
            total,
            unknown_count: self.result.unknown_count,
            inherited_uncertainty: self.result.inherited_uncertainty,
            completeness,
            condition: &self.result.condition,
            predicate_strategies: &self.result.predicate_strategies,
            sort_strategy: &self.result.sort_strategy,
            sort_basis: self.result.sort_basis,
            sort_descending: self.result.sort_descending,
            limitations: self.result.limitations,
        }
    }
}

/// Read-only metadata bound to an admitted page.
pub struct Metadata<'a> {
    pub version: &'a Id,
    pub observed_at: Option<&'a Timestamp>,
    /// Immediate-child scope, or `None` for a full-version search.
    pub folder: Option<u32>,
    pub total: Total,
    pub unknown_count: u64,
    /// The parent selection had additional, unresolved candidates outside these rows.
    pub inherited_uncertainty: bool,
    pub completeness: ResultCompleteness,
    pub condition: &'a Condition,
    pub predicate_strategies: &'a [(String, u32)],
    pub sort_strategy: &'a (String, u32),
    pub sort_basis: Basis,
    pub sort_descending: bool,
    pub limitations: &'static [&'static str],
}
