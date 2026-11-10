//! Internal, read-only folder browsing over an admitted immutable result version.
//!
//! The desktop adapter shares this registry between folder opens and pages.
mod admission;
pub(crate) mod commands;
pub mod cursor;
pub mod job;
mod rows;
#[cfg(test)]
mod test_gate;
use rows::project;
mod wire;
pub use admission::{AdmissionError, LiveOpen};
use std::sync::Arc;

use du_analysis_index::{
    IngestSink, QueryError, ResultVersion, browse,
    query::{
        self,
        page::{MultiFolderKey, Page, PageError, ResultSets, SourceAdmission, Total},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeValue, Timestamp,
    result::{ResultCompleteness, ResultItem, ResultVersionRead},
};
use du_job_runtime::JobIdentity;

/// One supported folder view. Tree branches include only known directories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    List,
    Tree,
}

/// Caller-selected, already registered sort strategy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sort {
    pub id: String,
    pub revision: u32,
    pub basis: query::Basis,
    pub descending: bool,
}

/// Input for one folder within the source's result-version root.
#[derive(Clone, Debug)]
pub struct OpenRequest {
    pub result_version: Option<Id>,
    pub location: Option<ItemRef>,
    pub view: View,
    pub condition: query::Condition,
    pub sort: Sort,
    pub page_size: Option<u32>,
}

/// A breadcrumb's stable reference and observed native name bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Breadcrumb {
    pub item: ItemRef,
    pub name: Vec<u8>,
    /// Uses the observed absolute path for a real scope root.
    pub display_name: Option<String>,
}

/// A returned observation and its independently observed size states.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub item: ItemRef,
    pub observation: ResultItem,
    /// Name decoded using the observed root's native encoding.
    pub display_name: Option<String>,
    pub logical_size: Option<SizeValue>,
    pub allocated_size: Option<SizeValue>,
    /// Modification time retained for this item in the pinned observation.
    pub modified_at: Option<Timestamp>,
    /// Evidence that an immediate child folder was observed; absence is unknown.
    pub has_observed_child_folder: Option<bool>,
    /// Explicitly reported gaps at this exact observed item; empty does not prove completeness.
    pub reported_gaps: Vec<GapEvidence>,
}

/// A collector report attached to the exact observed item in a pinned result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapEvidence {
    pub reason_key: String,
    pub observed_at: Option<Timestamp>,
}

/// One page from a registered, pinned folder query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderPage {
    pub query_ref: Id,
    pub result_version: Id,
    pub rows: Vec<Row>,
    pub total: Total,
    /// Observed candidates for which the pinned condition could not be decided.
    pub unknown_count: u64,
    pub completeness: ResultCompleteness,
    pub observed_at: Option<Timestamp>,
    /// Next position within this query, absent when no observed rows remain.
    pub next_position: Option<u64>,
}

/// Opened location and first page of its immediate-child result set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    /// Absent at the virtual top of a multi-root result.
    pub location: Option<ItemRef>,
    pub breadcrumbs: Vec<Breadcrumb>,
    /// Sort strategies pinned to the query in priority order.
    pub sort_strategies: Vec<query::SortSpec>,
    /// Registered predicate strategies used to evaluate the pinned condition.
    pub predicate_strategies: Vec<(String, u32)>,
    pub page: FolderPage,
}

/// Internal errors; transport-specific error envelopes are intentionally deferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    MissingVersion,
    StaleVersion,
    MissingLocation,
    NotFolder,
    UnknownKind,
    InvalidPage,
    UnknownPredicate,
    UnknownSort,
    InvalidCondition,
    Cancelled,
    Internal,
}

/// Folder-query session over one publication point and immutable result registry.
pub struct FolderBrowse {
    sets: ResultSets,
    ids: Arc<dyn JobIdentity>,
}

struct AdmittedOpen {
    location: Option<ItemRef>,
    breadcrumbs: Vec<Breadcrumb>,
    query_ref: Id,
    page: Page,
}

impl AdmittedOpen {
    fn project(self) -> Result<Opened, Error> {
        let metadata = self.page.metadata();
        Ok(Opened {
            location: self.location,
            breadcrumbs: self.breadcrumbs,
            sort_strategies: metadata.sorts.to_vec(),
            predicate_strategies: metadata.predicate_strategies.to_vec(),
            page: project(self.query_ref, self.page)?,
        })
    }
}

#[derive(Clone)]
struct PreparedOpen {
    admission: SourceAdmission,
    version: Arc<ResultVersion>,
    location: Option<ItemRef>,
    child_count: usize,
    breadcrumbs: Vec<Breadcrumb>,
    key: MultiFolderKey,
    primary: Sort,
    additional: Vec<Sort>,
    page_size: Option<u32>,
}
enum Candidate {
    Cached(AdmittedOpen),
    Evaluated {
        prepared: Box<PreparedOpen>,
        result: query::QueryResult,
    },
}

impl PreparedOpen {
    fn estimated_work(&self) -> usize {
        self.child_count
            .saturating_mul(self.key.sorts.len().saturating_add(1))
    }
}

impl FolderBrowse {
    /// Creates a session; identifiers must be unique within this session.
    pub fn new(source: IngestSink, ids: Arc<dyn JobIdentity>) -> Self {
        Self {
            sets: ResultSets::new(source),
            ids,
        }
    }

    /// Pins the current version, opens one folder and returns its first page.
    pub fn open(
        &self,
        request: OpenRequest,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Opened, Error> {
        self.open_with_sorts(request, &[], cancelled)
    }

    /// Opens one folder with additional sort keys in request priority order.
    ///
    /// The request's original sort remains first. Repeated keys are retained
    /// because the published contract defines no duplicate-key rejection rule.
    pub fn open_with_sorts(
        &self,
        request: OpenRequest,
        additional: &[Sort],
        cancelled: impl FnMut() -> bool,
    ) -> Result<Opened, Error> {
        self.open_admitted(request, additional, cancelled)?
            .project()
    }

    fn prepare(&self, request: OpenRequest, additional: &[Sort]) -> Result<PreparedOpen, Error> {
        validate_count(request.page_size)?;
        let admission = self
            .sets
            .admit_source(request.result_version.as_ref())
            .map_err(page_error)?;
        let version = admission.version();
        let (location, breadcrumbs, child_count) =
            open_location(&version, request.location.as_ref())?;
        let condition = match request.view {
            View::List => request.condition,
            View::Tree => query::Condition::All(vec![
                request.condition,
                query::Condition::Predicate(query::Predicate::Kind(query::Kind::Directory)),
            ]),
        };
        query::validate_condition(&condition).map_err(query_error)?;
        let condition = condition.normalize();
        query::validate_condition(&condition).map_err(query_error)?;
        let validated_sorts: Vec<_> = std::iter::once(&request.sort)
            .chain(additional)
            .map(|sort| query::Sort {
                id: &sort.id,
                revision: sort.revision,
                basis: sort.basis,
                descending: sort.descending,
            })
            .collect();
        query::Registry::baseline()
            .validate_sorts(&validated_sorts)
            .map_err(query_error)?;
        let sorts = std::iter::once(&request.sort)
            .chain(additional)
            .map(|sort| query::SortSpec {
                id: sort.id.clone(),
                revision: sort.revision,
                basis: sort.basis,
                descending: sort.descending,
            })
            .collect();
        let key = MultiFolderKey {
            version: version.version_id().clone(),
            folder: location
                .as_ref()
                .map_or(query::FolderLocation::ScopeRoots, |item| {
                    query::FolderLocation::Item(item.item_no)
                }),
            tree: request.view == View::Tree,
            condition,
            sorts,
        };
        Ok(PreparedOpen {
            admission,
            version,
            location,
            child_count,
            breadcrumbs,
            key,
            primary: request.sort,
            additional: additional.to_vec(),
            page_size: request.page_size,
        })
    }

    fn evaluate(
        &self,
        prepared: PreparedOpen,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Candidate, Error> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if let Some(page) = self.cached(&prepared)? {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            return Ok(Candidate::Cached(page));
        }
        self.evaluate_uncached(prepared, cancelled)
    }

    fn cached(&self, prepared: &PreparedOpen) -> Result<Option<AdmittedOpen>, Error> {
        let cached = match self.sets.find_multi_folder(&prepared.key) {
            Ok(id) => id,
            Err(PageError::StaleVersion) => None,
            Err(error) => return Err(page_error(error)),
        };
        if let Some(id) = cached {
            match self
                .sets
                .open_page(&id, &prepared.key.version, 0, prepared.page_size)
            {
                Ok(page) => {
                    return Ok(Some(AdmittedOpen {
                        location: prepared.location.clone(),
                        breadcrumbs: prepared.breadcrumbs.clone(),
                        query_ref: id,
                        page,
                    }));
                }
                Err(PageError::StaleVersion) => {}
                Err(error) => return Err(page_error(error)),
            }
        }
        Ok(None)
    }

    fn evaluate_uncached(
        &self,
        prepared: PreparedOpen,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Candidate, Error> {
        let extra: Vec<_> = prepared
            .additional
            .iter()
            .map(|sort| query::Sort {
                id: &sort.id,
                revision: sort.revision,
                basis: sort.basis,
                descending: sort.descending,
            })
            .collect();
        let result = query::run_folder_with_sorts(
            &query::Registry::baseline(),
            prepared.version.clone(),
            prepared.location.as_ref(),
            &prepared.key.condition,
            query::Sort {
                id: &prepared.primary.id,
                revision: prepared.primary.revision,
                basis: prepared.primary.basis,
                descending: prepared.primary.descending,
            },
            &extra,
            &mut cancelled,
        )
        .map_err(query_error)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(Candidate::Evaluated {
            prepared: Box::new(prepared),
            result,
        })
    }

    fn publish_candidate(&self, candidate: Candidate) -> Result<AdmittedOpen, Error> {
        match candidate {
            Candidate::Cached(page) => Ok(page),
            Candidate::Evaluated { prepared, result } => {
                let (query_ref, page) = self
                    .sets
                    .register_admitted_folder(
                        prepared.admission,
                        prepared.key,
                        self.ids.next_id(),
                        result,
                        prepared.page_size,
                    )
                    .map_err(page_error)?;
                Ok(AdmittedOpen {
                    location: prepared.location,
                    breadcrumbs: prepared.breadcrumbs,
                    query_ref,
                    page,
                })
            }
        }
    }

    fn open_admitted(
        &self,
        request: OpenRequest,
        additional: &[Sort],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<AdmittedOpen, Error> {
        validate_count(request.page_size)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let prepared = self.prepare(request, additional)?;
        let candidate = self.evaluate(prepared, &mut cancelled)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        self.publish_candidate(candidate)
    }

    /// Admits a bounded positional page against the same registered result.
    pub fn page(
        &self,
        query_ref: &Id,
        version: &Id,
        start: u64,
        count: Option<u32>,
    ) -> Result<FolderPage, Error> {
        let page = self
            .sets
            .open_page(query_ref, version, start, count)
            .map_err(page_error)?;
        project(query_ref.clone(), page)
    }
}

fn open_location(
    version: &ResultVersion,
    requested: Option<&ItemRef>,
) -> Result<(Option<ItemRef>, Vec<Breadcrumb>, usize), Error> {
    let view = if let Some(requested) = requested {
        browse::open(version, Some(requested)).map_err(folder_error)?
    } else {
        match browse::open_scope(version).map_err(folder_error)? {
            browse::ScopeView::Single(view) => view,
            browse::ScopeView::Forest { roots, .. } => return Ok((None, Vec::new(), roots.len())),
        }
    };
    let reference = |item_no| ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    };
    let breadcrumbs = view
        .breadcrumbs
        .into_iter()
        .map(|item| {
            let item_ref = reference(item.item_no);
            Breadcrumb {
                display_name: super::item_display_name(version, &item_ref, item),
                item: item_ref,
                name: item.name.clone(),
            }
        })
        .collect();
    Ok((
        Some(reference(view.location.item_no)),
        breadcrumbs,
        view.children.len(),
    ))
}

fn validate_count(count: Option<u32>) -> Result<(), Error> {
    if count.is_some_and(|n| n == 0 || n > query::page::MAX_PAGE_SIZE) {
        Err(Error::InvalidPage)
    } else {
        Ok(())
    }
}

fn folder_error(error: QueryError) -> Error {
    match error {
        QueryError::StaleVersion => Error::StaleVersion,
        QueryError::NotFound => Error::MissingLocation,
        QueryError::NotDirectory => Error::NotFolder,
        QueryError::KindUnknown => Error::UnknownKind,
        _ => Error::Internal,
    }
}
fn page_error(error: PageError) -> Error {
    match error {
        PageError::NoSource => Error::MissingVersion,
        PageError::StaleVersion => Error::StaleVersion,
        PageError::InvalidRange => Error::InvalidPage,
        _ => Error::Internal,
    }
}
fn query_error(error: query::Error) -> Error {
    match error {
        query::Error::UnknownPredicate => Error::UnknownPredicate,
        query::Error::UnknownSort => Error::UnknownSort,
        query::Error::InvalidCondition => Error::InvalidCondition,
        query::Error::Cancelled => Error::Cancelled,
        query::Error::Folder(error) | query::Error::Parent(error) => folder_error(error),
        _ => Error::Internal,
    }
}
