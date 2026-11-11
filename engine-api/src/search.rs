//! Internal Rust entry point for a GUI search query.
//!
//! This deliberately does not define or register a wire command. The app host can
//! call it with the region opened by the existing query-region use case.
use std::sync::Arc;

use du_analysis_index::query::{
    Condition, Error as QueryError, Registry, SortSpec,
    page::{Page, PageError, ResultSets},
    prepared::{PreparedSearch, PreparedSearchError, SearchInput},
};
use du_core_contracts::job::{Checkpoint, JobBody, JobContext, JobOutcome};
use du_core_contracts::result::ResultVersionRead;
use du_core_contracts::{Id, query_region_ipc::QueryRegionKind};
use du_job_runtime::{JobIdentity, RuntimeError};

use crate::{
    api::{CallerCtx, Channel},
    browse::folder::job::{ErrorKind as FolderJobError, FolderJobs},
    query_region::{RegionError, commands::Regions},
    workers::Workers,
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
    Busy,
    Runtime,
}

/// A search that crossed the asynchronous execution boundary.
/// The query reference is available only from the completed F24 job result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchJob {
    pub job_ref: Id,
    pub request_seq: u32,
    pub result_version: Id,
}

/// The existing F24 folder registration and its shared worker wake-up path.
pub(crate) struct SearchWorkers {
    jobs: Arc<FolderJobs>,
    workers: Arc<Workers>,
}

impl SearchWorkers {
    pub(crate) fn new(jobs: Arc<FolderJobs>, workers: Arc<Workers>) -> Self {
        Self { jobs, workers }
    }
}

/// Synchronous search execution sharing the desktop query regions and F24 runtime.
pub struct SearchEntry {
    sets: Arc<ResultSets>,
    regions: Arc<Regions>,
    ids: Arc<dyn JobIdentity>,
    registry: Registry,
    async_workers: SearchWorkers,
}

impl SearchEntry {
    pub(crate) fn new(
        sets: Arc<ResultSets>,
        regions: Arc<Regions>,
        ids: Arc<dyn JobIdentity>,
        async_workers: SearchWorkers,
    ) -> Self {
        Self {
            sets,
            regions,
            ids,
            registry: Registry::baseline(),
            async_workers,
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
                caller
                    .with_active(|| {
                        self.sets
                            .register_admitted_search(query_ref, candidate)
                            .map_err(page_error)
                    })
                    .ok_or(SearchError::Cancelled)?
            })
            .map_err(|_| SearchError::Cancelled)?
    }

    /// Admits a pinned evaluation and hands its cancellation to the query region and F24.
    ///
    /// The caller can abandon the request before admission. Once the region binds
    /// the job, use query cancellation or job control to stop the evaluation.
    /// No result-set reference is registered until F24 commits the completed job.
    pub fn execute_async(
        &self,
        request: SearchRequest,
        caller: &CallerCtx,
    ) -> Result<SearchJob, SearchError> {
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
        let result_version = admission.version().version_id().clone();
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
        let query_ref = self.ids.next_id();
        let binding = Arc::new(std::sync::Mutex::new(None));
        let candidate = Arc::new(std::sync::Mutex::new(None));
        let body: Arc<dyn JobBody> = Arc::new(SearchEvaluation {
            prepared: std::sync::Mutex::new(Some(prepared)),
            candidate: candidate.clone(),
            #[cfg(test)]
            result_version: result_version.clone(),
        });
        let sets = self.sets.clone();
        let output = query_ref.clone();
        let publish_binding = binding.clone();
        let admission_error = std::cell::Cell::new(None);
        #[cfg(test)]
        let publish_version = result_version.clone();
        let job_ref = self
            .async_workers
            .jobs
            .submit_search_region(
                caller.correlation_id.clone(),
                body,
                |job| {
                    let mut slot = binding.lock().map_err(|_| RuntimeError::Poisoned)?;
                    #[cfg(test)]
                    test_gate::pause_admission(&result_version)
                        .map_err(|_| RuntimeError::InvalidState)?;
                    if caller.is_cancelled() {
                        admission_error.set(Some(SearchError::Cancelled));
                        return Err(RuntimeError::InvalidState);
                    }
                    let value = self
                        .regions
                        .registry
                        .promote_job(
                            &self.regions.owner,
                            &request.region_ref,
                            QueryRegionKind::Search,
                            request.request_seq,
                            job,
                        )
                        .map_err(|error| {
                            admission_error.set(Some(match error {
                                RegionError::InvalidSequence => SearchError::InvalidRequest,
                                _ => SearchError::RegionUnavailable,
                            }));
                            RuntimeError::InvalidState
                        })?;
                    *slot = Some(value);
                    Ok(())
                },
                move || {
                    let candidate = candidate
                        .lock()
                        .map_err(|_| "support.internal_error".to_owned())?
                        .take()
                        .ok_or_else(|| "support.internal_error".to_owned())?;
                    let permit = publish_binding
                        .lock()
                        .map_err(|_| "support.internal_error".to_owned())?
                        .as_ref()
                        .map(
                            |(permit, _): &(
                                crate::query_region::Permit<QueryRegionKind>,
                                Option<Id>,
                            )| permit.clone(),
                        )
                        .ok_or_else(|| "support.internal_error".to_owned())?;
                    permit
                        .publish(false, || {
                            #[cfg(test)]
                            if test_gate::pause_publish(&publish_version).is_err() {
                                return Err("support.internal_error".to_owned());
                            }
                            sets.register_current_admitted_search(output.clone(), candidate)
                                .map(|_| output)
                                .map_err(|error| {
                                    match error {
                                        PageError::StaleVersion => "result.requery_required",
                                        _ => "support.internal_error",
                                    }
                                    .to_owned()
                                })
                        })
                        .map_err(|_| "support.internal_error".to_owned())?
                },
            )
            .map_err(|error| {
                admission_error
                    .get()
                    .unwrap_or_else(|| folder_job_error(error))
            })?;
        let previous = binding
            .lock()
            .map_err(|_| SearchError::Unavailable)?
            .as_mut()
            .and_then(|(_, previous)| previous.take());
        if let Some(job) = previous {
            let _ = self.regions.cancel_job(&job);
        }
        self.async_workers.workers.notify();
        Ok(SearchJob {
            job_ref,
            request_seq: request.request_seq,
            result_version,
        })
    }
}

struct SearchEvaluation {
    prepared: std::sync::Mutex<Option<PreparedSearch>>,
    candidate: Arc<std::sync::Mutex<Option<du_analysis_index::query::page::ValidatedSearch>>>,
    #[cfg(test)]
    result_version: Id,
}

impl JobBody for SearchEvaluation {
    fn kind_id(&self) -> &str {
        "query.evaluate"
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let prepared = self.prepared.lock().ok().and_then(|mut value| value.take());
        let Some(prepared) = prepared else {
            return failed("support.internal_error");
        };
        #[cfg(test)]
        if test_gate::pause_worker(&self.result_version).is_err() {
            return failed("support.internal_error");
        }
        let evaluated = prepared.evaluate(|| context.checkpoint() == Checkpoint::Cancel);
        let candidate = match evaluated {
            Ok(candidate) => candidate,
            Err(PreparedSearchError::Query(QueryError::Cancelled)) => return cancelled(),
            Err(PreparedSearchError::Page(PageError::StaleVersion)) => {
                return failed("result.requery_required");
            }
            Err(_) => return failed("support.internal_error"),
        };
        if context.checkpoint() == Checkpoint::Cancel {
            return cancelled();
        }
        match self.candidate.lock() {
            Ok(mut slot) => *slot = Some(candidate),
            Err(_) => return failed("support.internal_error"),
        }
        #[cfg(test)]
        if test_gate::pause_candidate(&self.result_version).is_err() {
            return failed("support.internal_error");
        }
        JobOutcome {
            counts: Default::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

fn cancelled() -> JobOutcome {
    JobOutcome {
        counts: Default::default(),
        completed_scope: false,
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
    }
}

fn failed(reason: &str) -> JobOutcome {
    JobOutcome {
        counts: Default::default(),
        completed_scope: false,
        failure_reason_key: Some(reason.to_owned()),
        unprocessed_summary: None,
        result_version_id: None,
    }
}

fn folder_job_error(error: FolderJobError) -> SearchError {
    match error {
        FolderJobError::Busy | FolderJobError::Runtime(RuntimeError::QueueFull) => {
            SearchError::Busy
        }
        FolderJobError::Runtime(_) => SearchError::Runtime,
        _ => SearchError::Unavailable,
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
mod test_gate;
#[cfg(test)]
mod test_helpers;
#[cfg(test)]
mod tests;
