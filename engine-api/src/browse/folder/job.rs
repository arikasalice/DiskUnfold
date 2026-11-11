//! Pinned folder evaluation on the F24 query publication boundary.
use super::{AdmittedOpen, Candidate, Error, FolderBrowse, OpenRequest, PreparedOpen, Sort};
use du_core_contracts::{
    Id,
    job::{
        Checkpoint, HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobOutcome,
        ProgressUnit, ResourceClass,
    },
};
use du_job_runtime::{JobPriority, JobRuntime, RuntimeError, Submission, SubmitResult};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Debug)]
pub enum ErrorKind {
    Folder(Error),
    Runtime(RuntimeError),
    Admission(super::AdmissionError),
    Cursor(super::cursor::CursorError),
    Unavailable,
    Busy,
}
#[derive(Default)]
struct Channels {
    by_name: HashMap<String, Id>,
    active: Vec<Id>,
}
/// Serializes replacement within each query channel and limits retained jobs.
pub struct FolderJobs {
    browse: Arc<FolderBrowse>,
    runtime: Arc<JobRuntime>,
    channels: Mutex<Channels>,
}
/// The first page remains pinned until consumed or this ticket is dropped.
pub struct FolderTicket {
    id: Id,
    runtime: Arc<JobRuntime>,
    first: Arc<Mutex<Option<AdmittedOpen>>>,
}
impl FolderTicket {
    pub fn id(&self) -> &Id {
        &self.id
    }
    pub fn take_first(&self) -> Result<Option<super::Opened>, ErrorKind> {
        let state = self.runtime.state(&self.id).map_err(ErrorKind::Runtime)?;
        if state != du_core_contracts::job::JobState::Done {
            return Ok(None);
        }
        self.first
            .lock()
            .map_err(|_| ErrorKind::Unavailable)?
            .take()
            .map(|value| value.project().map_err(ErrorKind::Folder))
            .transpose()
    }
}
/// A typed open retains the admitting session and request sequence.
pub struct LiveTicket {
    ticket: FolderTicket,
    session: Arc<super::cursor::Session>,
    request_seq: u32,
}
impl LiveTicket {
    pub fn id(&self) -> &Id {
        self.ticket.id()
    }
    pub fn take_first(&self) -> Result<Option<super::LiveOpen>, ErrorKind> {
        let state = self
            .ticket
            .runtime
            .state(&self.ticket.id)
            .map_err(ErrorKind::Runtime)?;
        if state != du_core_contracts::job::JobState::Done {
            return Ok(None);
        }
        let admitted = self
            .ticket
            .first
            .lock()
            .map_err(|_| ErrorKind::Unavailable)?
            .take();
        admitted
            .map(|value| {
                self.session
                    .finish_open(value)
                    .map(|page| super::LiveOpen {
                        request_seq: self.request_seq,
                        page,
                    })
                    .map_err(ErrorKind::Cursor)
            })
            .transpose()
    }
}
impl FolderJobs {
    pub fn new(browse: Arc<FolderBrowse>, runtime: Arc<JobRuntime>) -> Result<Self, RuntimeError> {
        runtime.register(descriptor())?;
        Ok(Self {
            browse,
            runtime,
            channels: Mutex::new(Channels::default()),
        })
    }
    /// The owner wakes a runtime worker after successful submission.
    pub fn submit(
        &self,
        channel: &str,
        correlation: Id,
        request: OpenRequest,
        sorts: &[Sort],
    ) -> Result<FolderTicket, ErrorKind> {
        let prepared = self
            .browse
            .prepare(request, sorts)
            .map_err(ErrorKind::Folder)?;
        self.submit_prepared(channel, correlation, prepared)
    }
    pub fn submit_live(
        &self,
        session: Arc<super::cursor::Session>,
        channel: &str,
        correlation: Id,
        request: du_core_contracts::browse_open_ipc::BrowseOpenRequest,
    ) -> Result<LiveTicket, ErrorKind> {
        if !session.uses_browse(&self.browse) {
            return Err(ErrorKind::Unavailable);
        }
        let (request_seq, request, sorts) =
            super::admission::lower_live(request).map_err(ErrorKind::Admission)?;
        let ticket = self.submit(channel, correlation, request, &sorts)?;
        Ok(LiveTicket {
            ticket,
            session,
            request_seq,
        })
    }
    fn cancel_previous(&self, channels: &Channels, channel: &str) -> Result<(), ErrorKind> {
        if let Some(previous) = channels.by_name.get(channel) {
            match self.runtime.cancel(previous) {
                Ok(_) | Err(RuntimeError::NotFound) => {}
                Err(error) => return Err(ErrorKind::Runtime(error)),
            }
        }
        Ok(())
    }

    pub(super) fn submit_prepared(
        &self,
        channel: &str,
        correlation: Id,
        prepared: PreparedOpen,
    ) -> Result<FolderTicket, ErrorKind> {
        let mut channels = self.channels.lock().map_err(|_| ErrorKind::Unavailable)?;
        reap(&self.runtime, &mut channels)?;
        self.cancel_previous(&channels, channel)?;
        reap(&self.runtime, &mut channels)?;
        if channels.active.len() >= 4 {
            return Err(ErrorKind::Busy);
        }
        let slot = Arc::new(Mutex::new(None));
        let first = Arc::new(Mutex::new(None));
        let body = Arc::new(Evaluate {
            browse: self.browse.clone(),
            prepared: Mutex::new(Some(prepared)),
            result: slot.clone(),
        });
        let browse = self.browse.clone();
        let output = first.clone();
        let result = self
            .runtime
            .submit_with_publication(
                Submission {
                    kind_id: "query.evaluate".into(),
                    deduplication_key: None,
                    body,
                    priority: JobPriority::Normal,
                    scope_summary: "folder query".into(),
                    correlation_id: correlation,
                    sample: false,
                },
                move || {
                    let candidate = slot
                        .lock()
                        .map_err(|_| "support.internal_error".to_owned())?
                        .take()
                        .ok_or_else(|| "support.internal_error".to_owned())?;
                    let mut first = output
                        .lock()
                        .map_err(|_| "support.internal_error".to_owned())?;
                    let admitted = browse.publish_candidate(candidate).map_err(|error| {
                        match error {
                            Error::StaleVersion => "result.requery_required",
                            _ => "support.internal_error",
                        }
                        .to_owned()
                    })?;
                    let id = admitted.page.query_ref().clone();
                    *first = Some(admitted);
                    Ok(id)
                },
            )
            .map_err(ErrorKind::Runtime)?;
        let id = match result {
            SubmitResult::Created(id) | SubmitResult::Existing(id) => id,
        };
        channels.by_name.insert(channel.to_owned(), id.clone());
        channels.active.push(id.clone());
        Ok(FolderTicket {
            id,
            runtime: self.runtime.clone(),
            first,
        })
    }

    /// Submits one region-bound evaluation without cancelling any other region.
    pub(super) fn submit_region(
        &self,
        correlation: Id,
        prepared: PreparedOpen,
        permit: crate::query_region::Permit<du_core_contracts::query_region_ipc::QueryRegionKind>,
    ) -> Result<FolderTicket, ErrorKind> {
        let mut channels = self.channels.lock().map_err(|_| ErrorKind::Unavailable)?;
        reap(&self.runtime, &mut channels)?;
        if channels.active.len() >= 4 {
            return Err(ErrorKind::Busy);
        }
        let slot = Arc::new(Mutex::new(None));
        let first = Arc::new(Mutex::new(None));
        let body = Arc::new(Evaluate {
            browse: self.browse.clone(),
            prepared: Mutex::new(Some(prepared)),
            result: slot.clone(),
        });
        let browse = self.browse.clone();
        let output = first.clone();
        let result = self
            .runtime
            .submit_with_publication(
                Submission {
                    kind_id: "query.evaluate".into(),
                    deduplication_key: None,
                    body,
                    priority: JobPriority::Normal,
                    scope_summary: "folder query".into(),
                    correlation_id: correlation,
                    sample: false,
                },
                move || {
                    permit
                        .publish(false, || {
                            let candidate = slot
                                .lock()
                                .map_err(|_| "support.internal_error".to_owned())?
                                .take()
                                .ok_or_else(|| "support.internal_error".to_owned())?;
                            let mut first = output
                                .lock()
                                .map_err(|_| "support.internal_error".to_owned())?;
                            let admitted =
                                browse.publish_candidate(candidate).map_err(|error| {
                                    match error {
                                        Error::StaleVersion => "result.requery_required",
                                        _ => "support.internal_error",
                                    }
                                    .to_owned()
                                })?;
                            let id = admitted.page.query_ref().clone();
                            *first = Some(admitted);
                            Ok(id)
                        })
                        .map_err(|_| "support.internal_error".to_owned())?
                },
            )
            .map_err(ErrorKind::Runtime)?;
        let id = match result {
            SubmitResult::Created(id) | SubmitResult::Existing(id) => id,
        };
        channels.active.push(id.clone());
        Ok(FolderTicket {
            id,
            runtime: self.runtime.clone(),
            first,
        })
    }

    /// Submits a second query kind on the same registered F24 query worker pool.
    /// The caller owns the candidate preparation and terminal publication policy.
    pub(crate) fn submit_search_region(
        &self,
        correlation: Id,
        body: Arc<dyn JobBody>,
        permit: crate::query_region::Permit<du_core_contracts::query_region_ipc::QueryRegionKind>,
        publish: impl FnOnce() -> Result<Id, String> + Send + 'static,
    ) -> Result<Id, ErrorKind> {
        let mut channels = self.channels.lock().map_err(|_| ErrorKind::Unavailable)?;
        reap(&self.runtime, &mut channels)?;
        if channels.active.len() >= 4 {
            return Err(ErrorKind::Busy);
        }
        let submitted = self
            .runtime
            .submit_with_publication_admission(
                Submission {
                    kind_id: "query.evaluate".into(),
                    deduplication_key: None,
                    body,
                    priority: JobPriority::Normal,
                    scope_summary: "search query".into(),
                    correlation_id: correlation,
                    sample: false,
                },
                |job| permit.bind_job(job).map_err(|_| RuntimeError::InvalidState),
                publish,
            )
            .map_err(ErrorKind::Runtime)?;
        let id = match submitted {
            SubmitResult::Created(id) | SubmitResult::Existing(id) => id,
        };
        channels.active.push(id.clone());
        Ok(id)
    }
}
fn reap(runtime: &JobRuntime, channels: &mut Channels) -> Result<(), ErrorKind> {
    let mut active = Vec::new();
    for id in &channels.active {
        match runtime.state(id) {
            Ok(state) if !state.is_terminal() => active.push(id.clone()),
            Ok(_) | Err(RuntimeError::NotFound) => {}
            Err(error) => return Err(ErrorKind::Runtime(error)),
        }
    }
    channels.by_name.retain(|_, id| active.contains(id));
    channels.active = active;
    Ok(())
}
fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: "query.evaluate".into(),
        version: 1,
        display_name_key: "query.evaluate".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::Compute],
        history: HistoryMode::None,
    }
}
struct Evaluate {
    browse: Arc<FolderBrowse>,
    prepared: Mutex<Option<PreparedOpen>>,
    result: Arc<Mutex<Option<Candidate>>>,
}
impl JobBody for Evaluate {
    fn kind_id(&self) -> &str {
        "query.evaluate"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let result = self
            .prepared
            .lock()
            .ok()
            .and_then(|mut prepared| prepared.take())
            .ok_or(Error::Internal)
            .and_then(|prepared| {
                #[cfg(test)]
                super::test_gate::pause_worker(&prepared.key.version)
                    .map_err(|_| Error::Internal)?;
                self.browse
                    .evaluate(prepared, || context.checkpoint() == Checkpoint::Cancel)
            });
        let error = match result {
            Ok(candidate) => match self.result.lock() {
                Ok(mut result) => {
                    *result = Some(candidate);
                    None
                }
                Err(_) => Some("support.internal_error"),
            },
            Err(Error::Cancelled) => None,
            Err(Error::StaleVersion) => Some("result.requery_required"),
            Err(Error::MissingLocation) => Some("browse.location_missing"),
            Err(Error::NotFolder) => Some("browse.not_folder"),
            Err(Error::UnknownSort) => Some("sort.key_unknown"),
            Err(Error::UnknownPredicate) => Some("predicate.unknown"),
            Err(_) => Some("support.internal_error"),
        };
        JobOutcome {
            counts: Default::default(),
            completed_scope: error.is_none() && context.checkpoint() == Checkpoint::Continue,
            failure_reason_key: error.map(str::to_owned),
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
