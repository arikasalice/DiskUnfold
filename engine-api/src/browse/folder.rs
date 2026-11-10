//! Internal, read-only folder browsing over an admitted immutable result version.
//!
//! This API does not define or register the `browse.open` or `query.page` wire
//! contracts. The caller supplies identifiers and typed conditions, and owns
//! transport, session composition and result-source resolution.
pub mod cursor;
use std::sync::Arc;

use du_analysis_index::{
    IngestSink, QueryError, ResultVersion, browse,
    query::{
        self,
        page::{MultiFolderKey, Page, PageError, ResultSets, Total},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeBasis, SizeValue, Timestamp,
    result::{ResultCompleteness, ResultItem, ResultItemKind, ResultVersionRead},
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
    source: IngestSink,
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
        Ok(Opened {
            location: self.location,
            breadcrumbs: self.breadcrumbs,
            page: project(self.query_ref, self.page)?,
        })
    }
}

impl FolderBrowse {
    /// Creates a session; identifiers must be unique within this session.
    pub fn new(source: IngestSink, ids: Arc<dyn JobIdentity>) -> Self {
        Self {
            sets: ResultSets::new(source.clone()),
            source,
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
        let version = match &request.result_version {
            Some(id) => self.source.pin_current(id).map_err(source_error)?,
            None => self
                .source
                .pin()
                .map_err(|_| Error::Internal)?
                .ok_or(Error::MissingVersion)?,
        };
        let (location, breadcrumbs) = open_location(&version, request.location.as_ref())?;
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
        let sorts: Vec<_> = std::iter::once(&request.sort)
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
        if let Some(id) = self.sets.find_multi_folder(&key).map_err(page_error)? {
            match self.sets.open_page(&id, &key.version, 0, request.page_size) {
                Ok(page) => {
                    if cancelled() {
                        return Err(Error::Cancelled);
                    }
                    return Ok(AdmittedOpen {
                        location,
                        breadcrumbs,
                        query_ref: id,
                        page,
                    });
                }
                Err(PageError::StaleVersion) => {}
                Err(error) => return Err(page_error(error)),
            }
        }
        let extra: Vec<_> = additional
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
            version,
            location.as_ref(),
            &key.condition,
            query::Sort {
                id: &request.sort.id,
                revision: request.sort.revision,
                basis: request.sort.basis,
                descending: request.sort.descending,
            },
            &extra,
            cancelled,
        )
        .map_err(query_error)?;
        let id = self
            .sets
            .register_multi_folder(key.clone(), self.ids.next_id(), result)
            .map_err(page_error)?;
        let page = self
            .sets
            .open_page(&id, &key.version, 0, request.page_size)
            .map_err(page_error)?;
        Ok(AdmittedOpen {
            location,
            breadcrumbs,
            query_ref: id,
            page,
        })
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
) -> Result<(Option<ItemRef>, Vec<Breadcrumb>), Error> {
    let view = if let Some(requested) = requested {
        browse::open(version, Some(requested)).map_err(folder_error)?
    } else {
        match browse::open_scope(version).map_err(folder_error)? {
            browse::ScopeView::Single(view) => view,
            browse::ScopeView::Forest { .. } => return Ok((None, Vec::new())),
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
    Ok((Some(reference(view.location.item_no)), breadcrumbs))
}

fn validate_count(count: Option<u32>) -> Result<(), Error> {
    if count.is_some_and(|n| n == 0 || n > query::page::MAX_PAGE_SIZE) {
        Err(Error::InvalidPage)
    } else {
        Ok(())
    }
}

fn project(query_ref: Id, page: Page) -> Result<FolderPage, Error> {
    let version = page.version();
    let rows = page
        .items()
        .iter()
        .map(|reference| {
            let item = version.detail(reference).map_err(folder_error)?.item;
            Ok(Row {
                item: reference.clone(),
                display_name: super::item_display_name(version, reference, item),
                observation: item.clone(),
                logical_size: version.item_size(item.item_no, SizeBasis::Logical).cloned(),
                allocated_size: version
                    .item_size(item.item_no, SizeBasis::Allocated)
                    .cloned(),
                modified_at: version.item_modified_at(item.item_no).cloned(),
                reported_gaps: version
                    .item_scope_gaps(reference)
                    .map_err(folder_error)?
                    .map(|gap| GapEvidence {
                        reason_key: gap.reason_key.clone(),
                        observed_at: gap.observed_at.clone(),
                    })
                    .collect(),
                has_observed_child_folder: if item.kind == ResultItemKind::Directory {
                    browse::has_observed_child_directory(version, reference)
                        .map_err(folder_error)?
                        .then_some(true)
                } else {
                    None
                },
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let metadata = page.metadata();
    let known_count = match metadata.total {
        Total::Exact(n) | Total::LowerBound(n) => n,
    };
    let next = u64::try_from(page.next_position()).map_err(|_| Error::Internal)?;
    Ok(FolderPage {
        query_ref,
        result_version: metadata.version.clone(),
        rows,
        total: metadata.total,
        completeness: metadata.completeness,
        observed_at: metadata.observed_at.cloned(),
        next_position: (next < known_count).then_some(next),
    })
}

fn source_error(error: QueryError) -> Error {
    match error {
        QueryError::NotFound => Error::MissingVersion,
        QueryError::StaleVersion => Error::StaleVersion,
        _ => Error::Internal,
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
