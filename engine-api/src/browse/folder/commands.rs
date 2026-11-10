//! GUI-only folder commands over the desktop scan store and F24 workers.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use du_analysis_index::IngestSink;
use du_core_contracts::{
    browse_open_ipc::BrowseOpenRequest,
    browse_page_ipc::{BrowseOpenResponse, QueryPageResponse, ResultOrigin},
    ports::Random,
    query_page_ipc::QueryPageRequest,
};
use du_job_runtime::{JobIdentity, RuntimeError};

use super::{
    AdmissionError, Error, FolderBrowse, LiveOpen, PreparedOpen,
    admission::lower_live,
    cursor::{CursorError, Session},
    job::{ErrorKind, FolderJobs},
};
use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    workers::Workers,
};

const BROWSE_CHANNEL: &str = "browse.main";
const SYNC_LIMIT: Duration = Duration::from_millis(300);
// Conservative preflight before a measured per-device cost model is available.
const SYNC_WORK: usize = 4096;

/// Composes both browse commands over the same scan-only publication store.
pub(crate) fn register(
    builder: &mut EngineBuilder,
    store: IngestSink,
    ids: Arc<dyn JobIdentity>,
    workers: Arc<Workers>,
    random: &dyn Random,
) -> Result<(), RuntimeError> {
    let browse = Arc::new(FolderBrowse::new(store, ids));
    let Ok(session) = Session::try_new(browse.clone(), random) else {
        return Ok(());
    };
    let session = Arc::new(session);
    let jobs = FolderJobs::new(browse.clone(), workers.runtime.clone())?;
    builder.register(Open {
        browse,
        session: session.clone(),
        jobs,
        workers,
        admission: Mutex::new(None),
    });
    builder.register(Page(session));
    Ok(())
}

struct Open {
    browse: Arc<FolderBrowse>,
    session: Arc<Session>,
    jobs: FolderJobs,
    workers: Arc<Workers>,
    admission: Mutex<Option<Arc<AtomicBool>>>,
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
        let (request_seq, request, sorts) = lower_live(request).map_err(admission_error)?;
        let token = self.admit()?;
        #[cfg(test)]
        tests::pause_at(request_seq, 1);
        let prepared = self.browse.prepare(request, &sorts).map_err(folder_error)?;
        if let Some(page) = self.browse.cached(&prepared).map_err(folder_error)? {
            #[cfg(test)]
            tests::pause_at(request_seq, 2);
            return self.with_current(&token, || self.opened(request_seq, page));
        }
        if prepared.estimated_work() <= SYNC_WORK && start.elapsed() < SYNC_LIMIT {
            let ended = || {
                token.load(Ordering::Acquire)
                    || caller.is_cancelled()
                    || start.elapsed() >= SYNC_LIMIT
            };
            match self.browse.evaluate_uncached(prepared.clone(), ended) {
                Ok(candidate) if !ended() => {
                    #[cfg(test)]
                    tests::pause_at(request_seq, 3);
                    return self.with_current(&token, || {
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
        self.with_current(&token, || self.enqueue(request_seq, caller, prepared))
    }
}

impl Open {
    fn admit(&self) -> Result<Arc<AtomicBool>, EngineError> {
        let mut current = self
            .admission
            .lock()
            .map_err(|_| folder_error(Error::Internal))?;
        self.jobs
            .cancel_channel(BROWSE_CHANNEL)
            .map_err(job_error)?;
        if let Some(previous) = current.as_ref() {
            previous.store(true, Ordering::Release);
        }
        let token = Arc::new(AtomicBool::new(false));
        *current = Some(token.clone());
        Ok(token)
    }

    fn with_current<T>(
        &self,
        token: &AtomicBool,
        action: impl FnOnce() -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let _guard = self
            .admission
            .lock()
            .map_err(|_| folder_error(Error::Internal))?;
        if token.load(Ordering::Acquire) {
            return Err(folder_error(Error::Cancelled));
        }
        action()
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
        request_seq: u32,
        caller: &CallerCtx,
        prepared: PreparedOpen,
    ) -> Result<BrowseOpenResponse, EngineError> {
        let ticket = self
            .jobs
            .submit_prepared(BROWSE_CHANNEL, caller.correlation_id.clone(), prepared)
            .map_err(job_error)?;
        self.workers.notify();
        Ok(BrowseOpenResponse::Job {
            request_seq,
            job_ref: ticket.id().clone(),
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
        self.0
            .read_request(request)
            .map_err(|error| cursor_error(error, continuation))?
            .into_response()
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
        ErrorKind::Busy | ErrorKind::Runtime(RuntimeError::QueueFull) => {
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
