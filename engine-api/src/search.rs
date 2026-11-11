//! Internal Rust entry point for a GUI search query.
//!
//! This deliberately does not define or register a wire command. The app host can
//! call it with the region opened by the existing query-region use case. This
//! synchronous helper is not connected to normal GUI search dispatch; do not route
//! user-facing queries through it until the F24 asynchronous path is connected.
use std::sync::Arc;

use du_analysis_index::query::{
    Condition, Error as QueryError, Registry, SortSpec,
    page::{Page, PageError, ResultSets},
    prepared::{PreparedSearch, PreparedSearchError, SearchInput},
};
use du_core_contracts::{Id, query_region_ipc::QueryRegionKind};
use du_job_runtime::JobIdentity;

use crate::{
    api::{CallerCtx, Channel},
    query_region::commands::Regions,
};

/// Search inputs for the internal engine API. This is not a shared IPC type.
pub struct SearchRequest {
    pub region_ref: Id,
    pub request_seq: u32,
    pub result_version: Option<Id>,
    pub parent_search_ref: Option<Id>,
    pub condition: Condition,
    pub sorts: Vec<SortSpec>,
    pub count: Option<u32>,
}

/// Internal failures before the transport owner maps them to a response envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchError {
    WrongChannel,
    RegionUnavailable,
    InvalidRequest,
    StaleVersion,
    NoSource,
    Cancelled,
    InvalidResult,
    Unavailable,
}

/// Synchronous search execution sharing the desktop query regions and F24 runtime.
pub struct SearchEntry {
    sets: Arc<ResultSets>,
    regions: Arc<Regions>,
    ids: Arc<dyn JobIdentity>,
    registry: Registry,
}

impl SearchEntry {
    pub(crate) fn new(
        sets: Arc<ResultSets>,
        regions: Arc<Regions>,
        ids: Arc<dyn JobIdentity>,
    ) -> Self {
        Self {
            sets,
            regions,
            ids,
            registry: Registry::baseline(),
        }
    }

    /// Validates, promotes, evaluates and publishes a search under its query-region fence.
    ///
    /// Cancellation, source replacement, invalid conditions and invalid candidates do not
    /// replace or release an already registered result. This may evaluate the full result
    /// version synchronously and is not registered with normal GUI dispatch. Long evaluations
    /// belong on the F24 asynchronous path, which is not exposed by this helper.
    pub fn execute(&self, request: SearchRequest, caller: &CallerCtx) -> Result<Page, SearchError> {
        if caller.channel != Channel::Gui {
            return Err(SearchError::WrongChannel);
        }
        self.regions
            .registry
            .validate_region(
                &self.regions.owner,
                &request.region_ref,
                QueryRegionKind::Search,
            )
            .map_err(|_| SearchError::RegionUnavailable)?;
        if caller.is_cancelled() {
            return Err(SearchError::Cancelled);
        }

        let admission = self
            .sets
            .admit_search(
                request.result_version.as_ref(),
                request.parent_search_ref.as_ref(),
            )
            .map_err(page_error)?;
        let prepared = PreparedSearch::new(
            self.sets.clone(),
            admission,
            &self.registry,
            SearchInput {
                condition: request.condition,
                sorts: request.sorts,
                count: request.count,
            },
        )
        .map_err(prepared_error)?;
        if caller.is_cancelled() {
            return Err(SearchError::Cancelled);
        }
        let promotion = self
            .regions
            .registry
            .promote_validated(
                &self.regions.owner,
                &request.region_ref,
                QueryRegionKind::Search,
                request.request_seq,
                Ok::<_, ()>(prepared),
            )
            .map_err(|_| SearchError::RegionUnavailable)?
            .map_err(|()| SearchError::InvalidRequest)?;
        if let Some(job) = promotion.previous_job {
            let _ = self.regions.cancel_job(&job);
        }

        let permit = promotion.permit;
        let candidate = promotion
            .prepared
            .evaluate(|| permit.is_stopped() || caller.is_cancelled())
            .map_err(prepared_error)?;
        if permit.is_stopped() || caller.is_cancelled() {
            return Err(SearchError::Cancelled);
        }
        let query_ref = self.ids.next_id();
        permit
            .publish(true, || {
                if caller.is_cancelled() {
                    return Err(SearchError::Cancelled);
                }
                self.sets
                    .register_admitted_search(query_ref, candidate)
                    .map_err(page_error)
            })
            .map_err(|_| SearchError::Cancelled)?
    }
}

fn page_error(error: PageError) -> SearchError {
    match error {
        PageError::NoSource => SearchError::NoSource,
        PageError::StaleVersion => SearchError::StaleVersion,
        PageError::InvalidRange | PageError::DuplicateId | PageError::InvalidResult => {
            SearchError::InvalidResult
        }
        PageError::Unavailable => SearchError::Unavailable,
    }
}

fn prepared_error(error: PreparedSearchError) -> SearchError {
    match error {
        PreparedSearchError::Query(QueryError::Cancelled) => SearchError::Cancelled,
        PreparedSearchError::Query(_) => SearchError::InvalidRequest,
        PreparedSearchError::Page(error) => page_error(error),
    }
}

#[cfg(test)]
mod tests;
