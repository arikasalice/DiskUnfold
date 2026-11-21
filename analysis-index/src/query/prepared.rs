//! A movable search preparation bound to one immutable source and registry snapshot.
use std::sync::Arc;

use super::{
    Condition, Error, FolderLocation, Registry, Sort, SortSpec,
    page::{
        DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, PageError, ResultSets, SearchAdmission, ValidatedSearch,
    },
};
use du_core_contracts::result::{ResultItemKind, ResultVersionRead};

/// Failure to prepare or evaluate a pinned search.
#[derive(Debug)]
pub enum PreparedSearchError {
    /// The condition or strategy selection is invalid, or evaluation was cancelled.
    Query(Error),
    /// Source ownership, page size, or the candidate could not be validated.
    Page(PageError),
    /// The subtree scope is not a folder of the admitted version, or a refinement
    /// was combined with a subtree scope.
    ScopeUnavailable,
}

/// Caller-supplied search inputs, validated before the prepared search is created.
pub struct SearchInput {
    /// Normalized or normalizable condition to evaluate.
    pub condition: Condition,
    /// Ordered registered sort selections.
    pub sorts: Vec<SortSpec>,
    /// Requested first-page size; absent uses the standard default.
    pub count: Option<u32>,
}

/// A search ready to move into a job; its source and strategy snapshot cannot be changed.
pub struct PreparedSearch {
    sets: Arc<ResultSets>,
    admission: SearchAdmission,
    registry: Registry,
    condition: Condition,
    sorts: Vec<SortSpec>,
    count: u32,
    scope: Option<FolderLocation>,
}

impl PreparedSearch {
    /// Validates the complete request without traversing source items.
    /// Registered strategies are copied into a private snapshot; modifying the caller's
    /// registry afterwards cannot change this prepared search. A parent and additional
    /// condition are checked together before promotion.
    pub fn new(
        sets: Arc<ResultSets>,
        admission: SearchAdmission,
        registry: &Registry,
        input: SearchInput,
    ) -> Result<Self, PreparedSearchError> {
        let SearchInput {
            condition,
            sorts,
            count,
        } = input;
        if !admission.belongs_to(&sets) {
            return Err(PreparedSearchError::Page(PageError::InvalidResult));
        }
        let count = count.unwrap_or(DEFAULT_PAGE_SIZE);
        if count == 0 || count > MAX_PAGE_SIZE {
            return Err(PreparedSearchError::Page(PageError::InvalidRange));
        }
        super::plan::check_shape(&condition, 0).map_err(PreparedSearchError::Query)?;
        let condition = condition.normalize();
        let borrowed: Vec<_> = sorts.iter().map(borrow_sort).collect();
        registry
            .prepare_search(&condition, &borrowed)
            .map_err(PreparedSearchError::Query)?;
        if let Some(parent) = admission.parent() {
            super::plan::check_shape(&parent.condition, 1).map_err(PreparedSearchError::Query)?;
            super::plan::check_shape(&condition, 1).map_err(PreparedSearchError::Query)?;
            let combined = Condition::All(vec![parent.condition.clone(), condition.clone()]);
            super::plan::check_shape(&combined, 0).map_err(PreparedSearchError::Query)?;
            super::plan::check_shape(&combined.normalize(), 0)
                .map_err(PreparedSearchError::Query)?;
        }
        Ok(Self {
            sets,
            admission,
            registry: registry.clone(),
            condition,
            sorts,
            count,
            scope: None,
        })
    }

    /// Restricts candidates to the items below one folder at any depth, or below every
    /// scope root (the subtree access path of query.run). The folder or roots themselves
    /// are not candidates. A refinement of a parent search cannot be restricted.
    pub fn within(mut self, scope: FolderLocation) -> Result<Self, PreparedSearchError> {
        if self.admission.parent().is_some() {
            return Err(PreparedSearchError::ScopeUnavailable);
        }
        if let FolderLocation::Item(item) = scope {
            let version = self.admission.version();
            let folder = version
                .items()
                .get(item as usize)
                .ok_or(PreparedSearchError::ScopeUnavailable)?;
            if folder.kind != ResultItemKind::Directory {
                return Err(PreparedSearchError::ScopeUnavailable);
            }
        }
        self.scope = Some(scope);
        Ok(self)
    }

    /// Evaluates exactly the prepared search, then validates its entire candidate outside
    /// the caller's publication lock. The caller must fence publication against later cancellation.
    pub fn evaluate(
        self,
        cancelled: impl FnMut() -> bool,
    ) -> Result<ValidatedSearch, PreparedSearchError> {
        let borrowed: Vec<_> = self.sorts.iter().map(borrow_sort).collect();
        let (first, rest) = borrowed
            .split_first()
            .ok_or(PreparedSearchError::Query(Error::UnknownSort))?;
        let result = match (self.admission.parent(), self.scope) {
            (_, Some(scope)) => super::subtree::run_subtree_with_sorts(
                &self.registry,
                self.admission.version(),
                scope,
                &self.condition,
                *first,
                rest,
                cancelled,
            ),
            (Some(parent), None) => super::run_in_result_with_sorts(
                &self.registry,
                parent,
                &self.condition,
                *first,
                rest,
                cancelled,
            ),
            (None, None) => super::run_with_sorts(
                &self.registry,
                self.admission.version(),
                &self.condition,
                *first,
                rest,
                cancelled,
            ),
        }
        .map_err(PreparedSearchError::Query)?;
        self.sets
            .validate_search_candidate(self.admission, result, Some(self.count))
            .map_err(PreparedSearchError::Page)
    }
}

fn borrow_sort(sort: &SortSpec) -> Sort<'_> {
    Sort {
        id: &sort.id,
        revision: sort.revision,
        basis: sort.basis,
        descending: sort.descending,
    }
}
