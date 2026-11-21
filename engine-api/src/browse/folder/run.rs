//! GUI query.run (F04 IF-01): one condition over the items below a folder or below
//! every scope root, opened as a search page in a search query region.
//!
//! Small versions are evaluated within the request. Larger ones go to the F24
//! `query.evaluate` job; its completed status reports the query reference, and the
//! first page is read with `query.page`, as for `browse.open`.
use std::sync::Arc;

use du_analysis_index::query::{Basis, FolderLocation, SortSpec};
use du_core_contracts::{
    Id, ItemRef, SizeBasis,
    browse_open_ipc::BrowseSortKey,
    browse_page_ipc::ResultOrigin,
    query_page_ipc::{QueryPagePosition, QueryPageRequest},
    query_run_ipc::{QueryRunRequest, QueryRunResponse},
};

use super::{
    admission::lower_condition,
    cursor::{AdmittedQueryPage, Session},
};
use crate::{
    api::{CallerCtx, Channel, UseCase},
    error::EngineError,
    search::{SearchEntry, SearchError, SearchRequest},
    source_exposure::admit_source,
};

/// Versions with at most this many items are evaluated within the request.
/// A conservative preflight until a measured per-device cost model is available.
pub(super) const SYNC_ITEMS: usize = 20_000;
const DEFAULT_PAGE: u32 = 200;
const MAX_PAGE: u32 = 1000;

pub(super) struct Run {
    pub(super) entry: Arc<SearchEntry>,
    pub(super) session: Arc<Session>,
}

impl UseCase for Run {
    const NAME: &'static str = "query.run";
    // Rows carry display paths: never listed for CLI or MCP.
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = QueryRunRequest;
    type Response = QueryRunResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let count = request.page_size.unwrap_or(DEFAULT_PAGE);
        if count == 0 || count > MAX_PAGE {
            return Err(EngineError::reason("cursor.limit_exceeded"));
        }
        if request.source_ref.is_some() {
            return Err(EngineError::reason("ref.unavailable"));
        }
        let scope = request.scope.clone();
        let (expected, location) = location(request.result_version, request.scope)?;
        let search = SearchRequest {
            region_ref: request.region_ref,
            request_seq: request.request_seq,
            result_version: expected.clone(),
            parent_search_ref: None,
            condition: lower_condition(request.condition)
                .map_err(|_| EngineError::reason("query.syntax_invalid"))?,
            sorts: sorts(request.sort_keys.unwrap_or_default())?,
            count: Some(count),
            scope: Some(location),
        };
        let (items, source) = self
            .entry
            .admitted_version(expected.as_ref())
            .map_err(search_error)?;
        admit_source(source, caller)?;
        if items > SYNC_ITEMS {
            let job = self
                .entry
                .execute_async(search, caller)
                .map_err(search_error)?;
            return Ok(QueryRunResponse::Job {
                request_seq: job.request_seq,
                job_ref: job.job_ref,
                result_version: job.result_version,
            });
        }
        let request_seq = search.request_seq;
        let evaluated = self.entry.execute(search, caller).map_err(search_error)?;
        let result_version = evaluated.metadata().version.clone();
        let (page, source) = self
            .session
            .read_query_request(QueryPageRequest::Position(QueryPagePosition {
                query_ref: evaluated.query_ref().clone(),
                result_version: result_version.clone(),
                start: 0,
                count,
            }))
            .map_err(|_| EngineError::reason("support.internal_error"))?;
        admit_source(source, caller)?;
        let AdmittedQueryPage::Search(page) = page else {
            return Err(EngineError::reason("support.internal_error"));
        };
        Ok(QueryRunResponse::Opened {
            request_seq,
            result_version,
            origin: ResultOrigin("scan".into()),
            data_source: Some(source),
            scope,
            page: Box::new(page),
        })
    }
}

/// The version to admit and the subtree: a folder reference names its own version.
fn location(
    result_version: Option<Id>,
    scope: Option<ItemRef>,
) -> Result<(Option<Id>, FolderLocation), EngineError> {
    match scope {
        None => Ok((result_version, FolderLocation::ScopeRoots)),
        Some(item) if result_version.is_some_and(|version| version != item.result_version) => {
            Err(EngineError::reason("result.requery_required"))
        }
        Some(item) => Ok((
            Some(item.result_version),
            FolderLocation::Item(item.item_no),
        )),
    }
}

fn sorts(keys: Vec<BrowseSortKey>) -> Result<Vec<SortSpec>, EngineError> {
    if keys.is_empty() {
        return Ok(vec![SortSpec {
            id: "sort.path".into(),
            revision: 1,
            basis: Basis::Logical,
            descending: false,
        }]);
    }
    keys.into_iter()
        .map(|key| {
            let basis = match key.size_basis {
                None | Some(SizeBasis::Logical) => Basis::Logical,
                Some(SizeBasis::Allocated) => Basis::Allocated,
                Some(SizeBasis::UniquePhysical) => {
                    return Err(EngineError::reason("query.syntax_invalid"));
                }
            };
            Ok(SortSpec {
                id: key.id,
                revision: key.revision,
                basis,
                descending: key.descending,
            })
        })
        .collect()
}

fn search_error(error: SearchError) -> EngineError {
    EngineError::reason(match error {
        SearchError::InvalidRequest => "command.body_invalid",
        SearchError::InvalidPageSize => "cursor.limit_exceeded",
        SearchError::InvalidCondition => "query.syntax_invalid",
        SearchError::UnknownPredicate => "predicate.unknown",
        SearchError::UnknownSort => "sort.key_unknown",
        SearchError::RegionUnavailable | SearchError::ScopeUnavailable | SearchError::NoSource => {
            "ref.unavailable"
        }
        SearchError::StaleVersion => "result.requery_required",
        SearchError::Cancelled => "request.cancelled",
        SearchError::Busy => "query.busy",
        SearchError::WrongChannel
        | SearchError::InvalidResult
        | SearchError::Unavailable
        | SearchError::Runtime => "support.internal_error",
    })
}
