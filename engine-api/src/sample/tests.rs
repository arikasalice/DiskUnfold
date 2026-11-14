//! Fakes and the rig shared by the sample session tests.

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use du_analysis_index::SampleSessionKey;
use du_core_contracts::{
    Id,
    job::{JobBody, JobContext, JobCounts, JobOutcome, JobState},
    sample_ipc::{SampleClosed, SampleObservationRef, SampleSession, SampleStateChanged},
};
use du_job_runtime::{DeduplicationKey, JobPriority, RuntimeError, Submission, SubmitResult};

use super::{
    KIND, SampleBundleInfo, SampleEventSink, SampleJobs, SampleLoadReport, SampleLoader,
    SampleSessions,
};
use crate::error::EngineError;

mod embedded;
mod engine;
mod flow;
mod real;
mod structure;

pub(super) type Log = Arc<Mutex<Vec<&'static str>>>;

pub(super) fn job_id(n: u64) -> Id {
    Id::from_v7_bytes(uuid_bytes(n))
}

fn uuid_bytes(n: u64) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[8..16].copy_from_slice(&(n | 0x8000_0000_0000_0000).to_be_bytes());
    bytes
}

pub(super) struct Body;

impl JobBody for Body {
    fn kind_id(&self) -> &str {
        KIND
    }

    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

#[derive(Default)]
pub(super) struct Report(pub(super) Mutex<Vec<SampleObservationRef>>);

impl SampleLoadReport for Report {
    fn observations(&self) -> Vec<SampleObservationRef> {
        self.0.lock().map(|list| list.clone()).unwrap_or_default()
    }
}

pub(super) struct Loader {
    pub(super) open: Mutex<Result<SampleBundleInfo, &'static str>>,
    pub(super) released: Mutex<Vec<SampleSessionKey>>,
    pub(super) built: AtomicU64,
    pub(super) report: Arc<Report>,
    pub(super) release_count: u64,
    log: Log,
}

impl SampleLoader for Loader {
    fn open(&self) -> Result<SampleBundleInfo, &'static str> {
        self.open
            .lock()
            .map(|open| open.clone())
            .unwrap_or(Err("poisoned"))
    }

    fn job(&self, _: &SampleSessionKey) -> (Arc<dyn JobBody>, Arc<dyn SampleLoadReport>) {
        self.built.fetch_add(1, Ordering::Relaxed);
        (Arc::new(Body), self.report.clone())
    }

    fn release(&self, session: &SampleSessionKey) -> u64 {
        if let Ok(mut log) = self.log.lock() {
            log.push("release");
        }
        if let Ok(mut released) = self.released.lock() {
            released.push(session.clone());
        }
        self.release_count
    }
}

pub(super) struct Recorded {
    pub(super) kind_id: String,
    pub(super) sample: bool,
    pub(super) dedup: Option<DeduplicationKey>,
    pub(super) scope_summary: String,
    pub(super) priority: JobPriority,
}

pub(super) struct Jobs {
    pub(super) submitted: Mutex<Vec<Recorded>>,
    pub(super) cancelled: Mutex<Vec<Id>>,
    pub(super) fail: Mutex<Option<RuntimeError>>,
    /// Delivers a job end synchronously from inside submit or cancel, like a runtime may.
    pub(super) finish_on_submit: Mutex<Option<JobState>>,
    pub(super) finish_on_cancel: Mutex<Option<JobState>>,
    pub(super) sessions: OnceLock<Arc<SampleSessions>>,
    next: AtomicU64,
    log: Log,
}

impl Jobs {
    fn deliver(&self, job: &Id, state: &Mutex<Option<JobState>>) {
        let state = state.lock().ok().and_then(|state| *state);
        if let (Some(state), Some(sessions)) = (state, self.sessions.get()) {
            sessions.finished(job.clone(), state);
        }
    }
}

impl SampleJobs for Jobs {
    fn submit(&self, submission: Submission) -> Result<SubmitResult, RuntimeError> {
        if let Some(error) = self.fail.lock().ok().and_then(|fail| fail.clone()) {
            return Err(error);
        }
        let id = job_id(self.next.fetch_add(1, Ordering::Relaxed) + 1);
        if let Ok(mut list) = self.submitted.lock() {
            list.push(Recorded {
                kind_id: submission.kind_id,
                sample: submission.sample,
                dedup: submission.deduplication_key,
                scope_summary: submission.scope_summary,
                priority: submission.priority,
            });
        }
        self.deliver(&id, &self.finish_on_submit);
        Ok(SubmitResult::Created(id))
    }

    fn cancel(&self, job_ref: &Id) {
        if let Ok(mut log) = self.log.lock() {
            log.push("cancel");
        }
        if let Ok(mut list) = self.cancelled.lock() {
            list.push(job_ref.clone());
        }
        self.deliver(job_ref, &self.finish_on_cancel);
    }
}

#[derive(Default)]
pub(super) struct Sink(pub(super) Mutex<Vec<(SampleStateChanged, Option<Id>)>>);

impl SampleEventSink for Sink {
    fn publish(&self, body: SampleStateChanged, job_ref: Option<Id>) {
        if let Ok(mut list) = self.0.lock() {
            list.push((body, job_ref));
        }
    }
}

pub(super) struct Rig {
    pub(super) sessions: Arc<SampleSessions>,
    pub(super) loader: Arc<Loader>,
    pub(super) jobs: Arc<Jobs>,
    pub(super) sink: Arc<Sink>,
    pub(super) log: Log,
    pub(super) cid: Id,
}

pub(super) fn ok<T>(result: Result<T, EngineError>) -> Result<T, String> {
    result.map_err(|error| format!("{error:?}"))
}

pub(super) fn closed_error() -> EngineError {
    EngineError::reason("sample.session_closed")
}

impl Rig {
    pub(super) fn new() -> Self {
        let log = Log::default();
        let loader = Arc::new(Loader {
            open: Mutex::new(Ok(SampleBundleInfo {
                bundle_version: "b1".into(),
            })),
            released: Mutex::default(),
            built: AtomicU64::new(0),
            report: Arc::new(Report::default()),
            release_count: 3,
            log: log.clone(),
        });
        let jobs = Arc::new(Jobs {
            submitted: Mutex::default(),
            cancelled: Mutex::default(),
            fail: Mutex::default(),
            finish_on_submit: Mutex::default(),
            finish_on_cancel: Mutex::default(),
            sessions: OnceLock::new(),
            next: AtomicU64::new(0),
            log: log.clone(),
        });
        let sink = Arc::new(Sink::default());
        let sessions = Arc::new(SampleSessions::new(
            jobs.clone(),
            loader.clone(),
            sink.clone(),
        ));
        let _ = jobs.sessions.set(sessions.clone());
        Self {
            sessions,
            loader,
            jobs,
            sink,
            log,
            cid: job_id(999),
        }
    }

    pub(super) fn open(&self) -> Result<SampleSession, EngineError> {
        self.sessions.open(&self.cid)
    }

    pub(super) fn status(&self, session: &Id) -> Result<SampleSession, EngineError> {
        self.sessions.status(session)
    }

    pub(super) fn reset(&self, session: &Id) -> Result<SampleSession, EngineError> {
        self.sessions.reset(session, &self.cid)
    }

    pub(super) fn close(&self, session: &Id) -> Result<SampleClosed, EngineError> {
        self.sessions.close(session)
    }

    pub(super) fn finish(&self, job: &Id, state: JobState) {
        self.sessions.finished(job.clone(), state);
    }

    pub(super) fn fail_bundle(&self, reason: &'static str) {
        if let Ok(mut open) = self.loader.open.lock() {
            *open = Err(reason);
        }
    }

    pub(super) fn fix_bundle(&self) {
        if let Ok(mut open) = self.loader.open.lock() {
            *open = Ok(SampleBundleInfo {
                bundle_version: "b1".into(),
            });
        }
    }

    pub(super) fn events(&self) -> Vec<SampleStateChanged> {
        self.sink
            .0
            .lock()
            .map(|list| list.iter().map(|(body, _)| body.clone()).collect())
            .unwrap_or_default()
    }

    pub(super) fn submissions(&self) -> usize {
        self.jobs
            .submitted
            .lock()
            .map(|list| list.len())
            .unwrap_or(0)
    }

    pub(super) fn cancelled(&self) -> Vec<Id> {
        self.jobs
            .cancelled
            .lock()
            .map(|list| list.clone())
            .unwrap_or_default()
    }

    pub(super) fn releases(&self) -> usize {
        self.loader
            .released
            .lock()
            .map(|list| list.len())
            .unwrap_or(0)
    }

    pub(super) fn seed_observations(&self, count: u32) {
        if let Ok(mut list) = self.loader.report.0.lock() {
            *list = (0..count)
                .map(|index| SampleObservationRef {
                    index,
                    result_version: job_id(500 + u64::from(index)),
                    observed_at: None,
                })
                .collect();
        }
    }
}
