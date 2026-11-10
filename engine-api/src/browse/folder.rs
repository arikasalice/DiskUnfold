//! Internal, read-only folder browsing over an admitted immutable result version.
//!
//! This API does not define or register the `browse.open` or `query.page` wire
//! contracts. The caller supplies identifiers and typed conditions, and owns
//! any transport, session cursor and result-source resolution.
use std::sync::{Arc, Mutex};

use du_analysis_index::{
    IngestSink, QueryError, browse,
    query::{
        self,
        page::{Page, PageError, ResultSets, Total},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeBasis, SizeValue, Timestamp,
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
}

/// A returned observation and its independently observed size states.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub item: ItemRef,
    pub observation: ResultItem,
    pub logical_size: Option<SizeValue>,
    pub allocated_size: Option<SizeValue>,
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
    pub location: ItemRef,
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

#[derive(Clone, PartialEq, Eq)]
struct Key {
    version: Id,
    folder: u32,
    view: View,
    condition: query::Condition,
    sort: Sort,
}

/// Folder-query session over one publication point and immutable result registry.
pub struct FolderBrowse {
    source: IngestSink,
    sets: ResultSets,
    ids: Arc<dyn JobIdentity>,
    cache: Mutex<Vec<(Key, Id)>>,
}

impl FolderBrowse {
    /// Creates a session; identifiers must be unique within this session.
    pub fn new(source: IngestSink, ids: Arc<dyn JobIdentity>) -> Self {
        Self {
            sets: ResultSets::new(source.clone()),
            source,
            ids,
            cache: Mutex::new(Vec::new()),
        }
    }

    /// Pins the current version, opens one folder and returns its first page.
    pub fn open(
        &self,
        request: OpenRequest,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Opened, Error> {
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
        let view = browse::open(&version, request.location.as_ref()).map_err(folder_error)?;
        let location = ItemRef {
            result_version: version.version_id().clone(),
            item_no: view.location.item_no,
        };
        let breadcrumbs = view
            .breadcrumbs
            .iter()
            .map(|item| Breadcrumb {
                item: ItemRef {
                    result_version: version.version_id().clone(),
                    item_no: item.item_no,
                },
                name: item.name.clone(),
            })
            .collect();
        let condition = match request.view {
            View::List => request.condition,
            View::Tree => query::Condition::All(vec![
                request.condition,
                query::Condition::Predicate(query::Predicate::Kind(query::Kind::Directory)),
            ]),
        }
        .normalize();
        let key = Key {
            version: version.version_id().clone(),
            folder: location.item_no,
            view: request.view,
            condition,
            sort: request.sort,
        };
        let mut cache = self.cache.lock().map_err(|_| Error::Internal)?;
        if let Some((_, id)) = cache.iter().find(|(stored, _)| *stored == key) {
            match self.sets.open_page(id, &key.version, 0, request.page_size) {
                Ok(page) => {
                    return Ok(Opened {
                        location,
                        breadcrumbs,
                        page: project(id.clone(), page)?,
                    });
                }
                Err(PageError::StaleVersion) => {}
                Err(error) => return Err(page_error(error)),
            }
        }
        cache.retain(|(stored, _)| *stored != key);
        let result = query::run_folder(
            &query::Registry::baseline(),
            version,
            Some(&location),
            &key.condition,
            query::Sort {
                id: &key.sort.id,
                revision: key.sort.revision,
                basis: key.sort.basis,
                descending: key.sort.descending,
            },
            cancelled,
        )
        .map_err(query_error)?;
        let id = self.ids.next_id();
        self.sets.register(id.clone(), result).map_err(page_error)?;
        let page = self
            .sets
            .open_page(&id, &key.version, 0, request.page_size)
            .map_err(page_error)?;
        if cache.len() == 16 {
            cache.remove(0);
        }
        cache.push((key, id.clone()));
        Ok(Opened {
            location,
            breadcrumbs,
            page: project(id, page)?,
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
                observation: item.clone(),
                logical_size: version.item_size(item.item_no, SizeBasis::Logical).cloned(),
                allocated_size: version
                    .item_size(item.item_no, SizeBasis::Allocated)
                    .cloned(),
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
