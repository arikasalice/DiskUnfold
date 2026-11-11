//! GUI-only folder commands over the desktop scan store and F24 workers.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use du_analysis_index::query::page::ResultSets;
use du_core_contracts::{
    Id,
    browse_open_ipc::BrowseOpenRequest,
    browse_page_ipc::{BrowseOpenResponse, QueryPageResponse, ResultOrigin},
    ports::Random,
    query_page_ipc::QueryPageRequest,
    query_region_ipc::QueryRegionKind,
};
use du_job_runtime::{JobIdentity, RuntimeError};

use super::{
    AdmissionError, Error, FolderBrowse, LiveOpen, PreparedOpen,
    admission::lower_live,
    cursor::{AdmittedQueryPage, CursorError, Session},
    job::{ErrorKind, FolderJobs, RegionBinding},
};
use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    query_region::{
        Permit, RegionError,
        commands::{Regions, region_error},
    },
    workers::Workers,
};

const SYNC_LIMIT: Duration = Duration::from_millis(300);
// Conservative preflight before a measured per-device cost model is available.
const SYNC_WORK: usize = 4096;

/// Composes both browse commands over the same scan-only publication store.
pub(crate) fn register(
    builder: &mut EngineBuilder,
    sets: Arc<ResultSets>,
    ids: Arc<dyn JobIdentity>,
    workers: Arc<Workers>,
    random: &dyn Random,
) -> Result<Option<Arc<crate::search::SearchEntry>>, RuntimeError> {
    let browse = Arc::new(FolderBrowse::with_result_sets(sets.clone(), ids.clone()));
    let Ok(session) = Session::try_new(browse.clone(), random) else {
        return Ok(None);
    };
    let session = Arc::new(session);
    let jobs = Arc::new(FolderJobs::new(browse.clone(), workers.runtime.clone())?);
    let regions = Regions::register(builder, ids.clone(), random, workers.runtime.clone())?;
    builder.register(Open {
        browse,
        session: session.clone(),
        jobs: jobs.clone(),
        workers: workers.clone(),
        regions: regions.clone(),
    });
    builder.register(Page(session));
    Ok(Some(Arc::new(crate::search::SearchEntry::new(
        sets,
        regions,
        ids,
        crate::search::SearchWorkers::new(jobs, workers),
    ))))
}

struct Open {
    browse: Arc<FolderBrowse>,
    session: Arc<Session>,
    jobs: Arc<FolderJobs>,
    workers: Arc<Workers>,
    regions: Arc<Regions>,
}

struct RegionRequest {
    id: Id,
    kind: QueryRegionKind,
    sequence: u32,
}

impl UseCase for Open {
    const NAME: &'static str = "browse.open";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = BrowseOpenRequest;
    type Response = BrowseOpenResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let start = Instant::now();
        let region_ref = request.region_ref.clone();
        let kind = match request.view_kind {
            du_core_contracts::browse_open_ipc::BrowseViewKind::List => QueryRegionKind::BrowseList,
            du_core_contracts::browse_open_ipc::BrowseViewKind::TreeBranch => {
                QueryRegionKind::BrowseTreeBranch
            }
        };
        self.regions
            .registry
            .validate_region(&self.regions.owner, &region_ref, kind)
            .map_err(region_error)?;
        let (request_seq, request, sorts) = lower_live(request).map_err(admission_error)?;
        #[cfg(test)]
        tests::pause_at(request_seq, 1);
        let prepared = self.browse.prepare(request, &sorts).map_err(folder_error)?;
        if caller.is_cancelled() {
            return Err(folder_error(Error::Cancelled));
        }
        let region = RegionRequest {
            id: region_ref,
            kind,
            sequence: request_seq,
        };
        if let Some(page) = self.browse.cached(&prepared).map_err(folder_error)? {
            #[cfg(test)]
            tests::pause_at(request_seq, 2);
            let token = self.promote(&region)?;
            return self.with_current(&token, caller, || self.opened(request_seq, page));
        }
        #[cfg(test)]
        let force_sync = tests::force_sync(request_seq);
        #[cfg(not(test))]
        let force_sync = false;
        if prepared.estimated_work() <= SYNC_WORK && (force_sync || start.elapsed() < SYNC_LIMIT) {
            let ended = || caller.is_cancelled() || (!force_sync && start.elapsed() >= SYNC_LIMIT);
            match self.browse.evaluate_uncached(prepared.clone(), ended) {
                Ok(candidate) if !ended() => {
                    #[cfg(test)]
                    tests::pause_at(request_seq, 3);
                    let token = self.promote(&region)?;
                    return self.with_current(&token, caller, || {
                        if caller.is_cancelled() {
                            return Err(folder_error(Error::Cancelled));
                        }
                        let page = self
                            .browse
                            .publish_candidate(candidate)
                            .map_err(folder_error)?;
                        self.opened(request_seq, page)
                    });
                }
                Ok(_) | Err(Error::Cancelled) => {}
                Err(error) => return Err(folder_error(error)),
            }
        }
        if caller.is_cancelled() {
            return Err(folder_error(Error::Cancelled));
        }
        #[cfg(test)]
        tests::pause_at(request_seq, 4);
        self.enqueue(&region, caller, prepared)
    }
}

impl Open {
    fn promote(&self, region: &RegionRequest) -> Result<Permit<QueryRegionKind>, EngineError> {
        let promotion = self
            .regions
            .registry
            .promote_validated(
                &self.regions.owner,
                &region.id,
                region.kind,
                region.sequence,
                Ok::<_, ()>(()),
            )
            .map_err(region_error)?
            .map_err(|()| folder_error(Error::Internal))?;
        if let Some(job) = promotion.previous_job {
            let _ = self.regions.cancel_job(&job);
        }
        Ok(promotion.permit)
    }
    fn with_current<T>(
        &self,
        token: &Permit<QueryRegionKind>,
        caller: &CallerCtx,
        action: impl FnOnce() -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        if caller.is_cancelled() {
            return Err(folder_error(Error::Cancelled));
        }
        token.publish(true, action).map_err(region_error)?
    }

    fn opened(
        &self,
        request_seq: u32,
        page: super::AdmittedOpen,
    ) -> Result<BrowseOpenResponse, EngineError> {
        let page = self
            .session
            .finish_open(page)
            .map_err(|error| cursor_error(error, false))?;
        LiveOpen { request_seq, page }.into_response(ResultOrigin("scan".into()))
    }

    fn enqueue(
        &self,
        region: &RegionRequest,
        caller: &CallerCtx,
        prepared: PreparedOpen,
    ) -> Result<BrowseOpenResponse, EngineError> {
        if caller.is_cancelled() {
            return Err(folder_error(Error::Cancelled));
        }
        let result_version = prepared.key.version.clone();
        let binding: RegionBinding = Arc::new(std::sync::Mutex::new(None));
        let admission_error = std::cell::RefCell::new(None);
        let ticket = self
            .jobs
            .submit_region(
                caller.correlation_id.clone(),
                prepared,
                binding.clone(),
                |job| {
                    let mut slot = binding.lock().map_err(|_| RuntimeError::Poisoned)?;
                    if caller.is_cancelled() {
                        *admission_error.borrow_mut() = Some(folder_error(Error::Cancelled));
                        return Err(RuntimeError::InvalidState);
                    }
                    let value = self
                        .regions
                        .registry
                        .promote_job(
                            &self.regions.owner,
                            &region.id,
                            region.kind,
                            region.sequence,
                            job,
                        )
                        .map_err(|error| {
                            *admission_error.borrow_mut() = Some(match error {
                                RegionError::Unavailable => folder_error(Error::Cancelled),
                                other => region_error(other),
                            });
                            RuntimeError::InvalidState
                        })?;
                    *slot = Some(value);
                    Ok(())
                },
            )
            .map_err(|error| {
                admission_error
                    .borrow_mut()
                    .take()
                    .unwrap_or_else(|| job_error(error))
            })?;
        let previous = binding
            .lock()
            .map_err(|_| folder_error(Error::Internal))?
            .as_mut()
            .and_then(|(_, previous)| previous.take());
        if let Some(job) = previous {
            let _ = self.regions.cancel_job(&job);
        }
        self.workers.notify();
        #[cfg(test)]
        tests::pause_at(region.sequence, 5);
        Ok(BrowseOpenResponse::Job {
            request_seq: region.sequence,
            job_ref: ticket.id().clone(),
            result_version,
        })
    }
}

struct Page(Arc<Session>);

impl UseCase for Page {
    const NAME: &'static str = "query.page";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = QueryPageRequest;
    type Response = QueryPageResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let continuation = matches!(request, QueryPageRequest::Continuation(_));
        match self
            .0
            .read_query_request(request)
            .map_err(|error| cursor_error(error, continuation))?
        {
            AdmittedQueryPage::Folder(page) => page.into_response(),
            AdmittedQueryPage::Search(page) => Ok(QueryPageResponse::Search(page)),
        }
    }
}

fn admission_error(error: AdmissionError) -> EngineError {
    match error {
        AdmissionError::SourceUnresolved => EngineError::reason("ref.unavailable"),
        AdmissionError::InvalidCondition | AdmissionError::InvalidSizeBasis => {
            EngineError::reason("command.body_invalid")
        }
        AdmissionError::Browse(error) => cursor_error(error, false),
    }
}

fn folder_error(error: Error) -> EngineError {
    EngineError::reason(match error {
        Error::MissingLocation => "browse.location_missing",
        Error::MissingVersion | Error::UnknownKind => "ref.unavailable",
        Error::StaleVersion => "result.requery_required",
        Error::NotFolder => "browse.not_folder",
        Error::InvalidPage => "cursor.limit_exceeded",
        Error::UnknownPredicate => "predicate.unknown",
        Error::UnknownSort => "sort.key_unknown",
        Error::InvalidCondition => "command.body_invalid",
        Error::Cancelled | Error::Internal => "support.internal_error",
    })
}

fn cursor_error(error: CursorError, continuation: bool) -> EngineError {
    match error {
        CursorError::Folder(error) => folder_error(error),
        CursorError::Invalid => EngineError::reason("cursor.invalid"),
        CursorError::InvalidPage => EngineError::reason("cursor.limit_exceeded"),
        CursorError::Stale if continuation => EngineError::reason("cursor.stale"),
        CursorError::Stale => EngineError::reason("result.requery_required"),
        CursorError::FingerprintUnavailable => EngineError::reason("support.internal_error"),
    }
}

fn job_error(error: ErrorKind) -> EngineError {
    match error {
        ErrorKind::Busy
        | ErrorKind::Runtime(RuntimeError::QueueFull | RuntimeError::DeliveryBusy) => {
            EngineError::reason("query.busy")
        }
        ErrorKind::Runtime(RuntimeError::ShuttingDown) => EngineError::reason("job.shutting_down"),
        ErrorKind::Folder(error) => folder_error(error),
        ErrorKind::Admission(error) => admission_error(error),
        ErrorKind::Cursor(error) => cursor_error(error, false),
        _ => EngineError::reason("support.internal_error"),
    }
}

#[cfg(test)]
mod tests;
