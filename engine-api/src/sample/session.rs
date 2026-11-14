//! The single sample session and its transition table.
//!
//! One session exists per app session. Every command runs under one mutex, and every
//! transition adds 1 to `sequence` and publishes one `sample.state_changed`, so a response
//! always carries the sequence of the latest event. A combination outside this table leaves
//! the state unchanged and answers with the current state.
//!
//! | state       | open          | status        | reset                        | close                 | load job ends                     |
//! |-------------|---------------|---------------|------------------------------|-----------------------|-----------------------------------|
//! | (none)      | new session   | not found     | not found                    | not found             | ignored                           |
//! | preparing   | same session  | current       | current (same job)           | cancel, release, closed | done: active; partial/failed: unavailable; cancelled: ignored |
//! | resetting   | same session  | current       | current (same job)           | cancel, release, closed | as preparing                      |
//! | active      | same session  | current       | resetting, release, preparing or unavailable | release, closed | ignored |
//! | unavailable | same session  | current       | resetting, release, preparing or unavailable | release, closed | ignored |
//! | closed      | new session   | not found     | not found                    | same result again     | ignored                           |
//!
//! A new session whose job cannot be queued is never created. A load job that ends while
//! a command runs is applied right after that command, never inside it.

use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use du_analysis_index::SampleSessionKey;
use du_core_contracts::{
    Id, Timestamp,
    job::JobState,
    sample_ipc::{
        SAMPLE_LOAD_FAILED, SAMPLE_REAL_ANALYSIS_ACTION, SAMPLE_SESSION_CLOSED, SampleClosed,
        SampleObservationRef, SampleSession, SampleSessionState, SampleStateChanged,
    },
};
use du_job_runtime::{DeduplicationKey, JobPriority, RuntimeError, Submission, SubmitResult};

use super::{KIND, SampleEventSink, SampleJobs, SampleLoadReport, SampleLoader};
use crate::error::EngineError;

/// A terminal `scan.sample` event waiting to be applied to the session.
struct Finished {
    job_ref: Id,
    state: JobState,
}

/// Owns the sample session of this app session.
pub(crate) struct SampleSessions {
    jobs: Arc<dyn SampleJobs>,
    loader: Arc<dyn SampleLoader>,
    sink: Arc<dyn SampleEventSink>,
    slot: Mutex<Option<Session>>,
    finished: Mutex<Vec<Finished>>,
}

struct Session {
    session_ref: Id,
    key: SampleSessionKey,
    state: SampleSessionState,
    sequence: u64,
    bundle_version: Option<String>,
    job_ref: Option<Id>,
    report: Option<Arc<dyn SampleLoadReport>>,
    observations: Vec<SampleObservationRef>,
    plans: Vec<Id>,
    unavailable_reason: Option<&'static str>,
    released_versions: u64,
}

enum Launch {
    Loading {
        bundle_version: String,
        job_ref: Id,
        report: Arc<dyn SampleLoadReport>,
    },
    Unavailable(&'static str),
}

impl Session {
    fn new() -> Self {
        let session_ref = new_id();
        Self {
            key: SampleSessionKey::new(session_ref.clone()),
            session_ref,
            state: SampleSessionState::Closed,
            sequence: 0,
            bundle_version: None,
            job_ref: None,
            report: None,
            observations: Vec::new(),
            plans: Vec::new(),
            unavailable_reason: None,
            released_versions: 0,
        }
    }

    fn loading(&self) -> bool {
        matches!(
            self.state,
            SampleSessionState::Preparing | SampleSessionState::Resetting
        )
    }

    fn view(&self) -> SampleSession {
        let unavailable = self.state == SampleSessionState::Unavailable;
        SampleSession {
            session_ref: self.session_ref.clone(),
            state: self.state,
            sequence: self.sequence,
            bundle_version: self.bundle_version.clone(),
            job_ref: self.job_ref.clone(),
            observations: self.observations.clone(),
            plans: self.plans.clone(),
            unavailable_reason: self
                .unavailable_reason
                .filter(|_| unavailable)
                .map(str::to_owned),
            real_analysis_action: unavailable.then(|| SAMPLE_REAL_ANALYSIS_ACTION.to_owned()),
        }
    }

    /// Drops everything built for the session; the loader releases the result versions.
    fn discard_results(&mut self) {
        self.job_ref = None;
        self.report = None;
        self.observations.clear();
        self.plans.clear();
    }
}

impl SampleSessions {
    pub(crate) fn new(
        jobs: Arc<dyn SampleJobs>,
        loader: Arc<dyn SampleLoader>,
        sink: Arc<dyn SampleEventSink>,
    ) -> Self {
        Self {
            jobs,
            loader,
            sink,
            slot: Mutex::new(None),
            finished: Mutex::new(Vec::new()),
        }
    }

    /// Opens the session, or returns the one that is still open.
    ///
    /// A bundle that fails validation yields an `unavailable` session without a job. A job
    /// that cannot be queued fails the call and leaves no session behind.
    pub(crate) fn open(&self, correlation_id: &Id) -> Result<SampleSession, EngineError> {
        self.command(|slot| {
            if let Some(open) = slot
                .as_ref()
                .filter(|session| session.state != SampleSessionState::Closed)
            {
                return Ok(open.view());
            }
            let mut session = Session::new();
            let launch = self
                .launch(&session, correlation_id)
                .map_err(submit_error)?;
            self.settle(&mut session, launch);
            let view = session.view();
            *slot = Some(session);
            Ok(view)
        })
    }

    /// Returns the current session, or `sample.session_closed` for any other reference.
    pub(crate) fn status(&self, session_ref: &Id) -> Result<SampleSession, EngineError> {
        self.command(|slot| Ok(open_session(slot, session_ref)?.view()))
    }

    /// Reloads an active or unavailable session; a loading session answers unchanged.
    ///
    /// The old result versions are released before the bundle is validated again. If the new
    /// job cannot be queued the session becomes unavailable with `sample.load_failed`.
    pub(crate) fn reset(
        &self,
        session_ref: &Id,
        correlation_id: &Id,
    ) -> Result<SampleSession, EngineError> {
        self.command(|slot| {
            let session = open_session(slot, session_ref)?;
            if session.loading() {
                return Ok(session.view());
            }
            self.move_to(session, SampleSessionState::Resetting, None, None);
            session.discard_results();
            self.loader.release(&session.key);
            let launch = self
                .launch(session, correlation_id)
                .unwrap_or(Launch::Unavailable(SAMPLE_LOAD_FAILED));
            self.settle(session, launch);
            Ok(session.view())
        })
    }

    /// Closes the session from any state and repeats the same result when called again.
    ///
    /// The running job is cancelled before the result versions are released. The session is
    /// locked as closed whatever the release finds.
    pub(crate) fn close(&self, session_ref: &Id) -> Result<SampleClosed, EngineError> {
        self.command(|slot| {
            let session = slot
                .as_mut()
                .filter(|session| session.session_ref == *session_ref)
                .ok_or_else(session_closed)?;
            if session.state != SampleSessionState::Closed {
                let job_ref = session.job_ref.take();
                if let Some(job_ref) = &job_ref {
                    self.jobs.cancel(job_ref);
                }
                session.released_versions = self.loader.release(&session.key);
                session.discard_results();
                self.move_to(session, SampleSessionState::Closed, None, job_ref);
            }
            Ok(SampleClosed {
                session_ref: session.session_ref.clone(),
                state: SampleSessionState::Closed,
                released_versions: session.released_versions,
            })
        })
    }

    /// Queues the end of a `scan.sample` job and applies it as soon as no command runs.
    ///
    /// Safe to call from the job runtime's event path, including while a command that
    /// triggered the event is still running.
    pub(crate) fn finished(&self, job_ref: Id, state: JobState) {
        if let Ok(mut queue) = self.finished.lock() {
            queue.push(Finished { job_ref, state });
        }
        self.drain();
    }

    fn command<T>(
        &self,
        run: impl FnOnce(&mut Option<Session>) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let result = {
            let mut slot = self.slot.lock().map_err(|_| internal())?;
            run(&mut slot)
        };
        self.drain();
        result
    }

    /// Applies queued job ends. Whoever releases the lock re-checks the queue, so an end
    /// queued while another thread holds the lock is never left behind.
    fn drain(&self) {
        loop {
            {
                let Ok(mut slot) = self.slot.try_lock() else {
                    return;
                };
                let batch = match self.finished.lock() {
                    Ok(mut queue) => std::mem::take(&mut *queue),
                    Err(_) => return,
                };
                for done in batch {
                    self.apply(&mut slot, done);
                }
            }
            if self.finished.lock().is_ok_and(|queue| queue.is_empty()) {
                return;
            }
        }
    }

    fn apply(&self, slot: &mut Option<Session>, done: Finished) {
        let Some(session) = slot.as_mut() else {
            return;
        };
        if !session.loading() || session.job_ref.as_ref() != Some(&done.job_ref) {
            return;
        }
        match done.state {
            JobState::Done => {
                let mut observations = session
                    .report
                    .take()
                    .map(|report| report.observations())
                    .unwrap_or_default();
                observations.sort_by_key(|observation| observation.index);
                session.observations = observations;
                session.job_ref = None;
                self.move_to(
                    session,
                    SampleSessionState::Active,
                    None,
                    Some(done.job_ref),
                );
            }
            JobState::Partial | JobState::Failed => {
                self.loader.release(&session.key);
                self.fail(session, SAMPLE_LOAD_FAILED, Some(done.job_ref));
            }
            // A cancelled job only follows a close or a replaced session.
            _ => {}
        }
    }

    /// Validates the bundle and submits the load job for `session`.
    fn launch(&self, session: &Session, correlation_id: &Id) -> Result<Launch, RuntimeError> {
        let info = match self.loader.open() {
            Ok(info) => info,
            Err(reason) => return Ok(Launch::Unavailable(reason)),
        };
        let (body, report) = self.loader.job(&session.key);
        let submission = Submission {
            kind_id: KIND.into(),
            deduplication_key: Some(DeduplicationKey {
                scope: session.session_ref.as_str().to_owned(),
                input: info.bundle_version.clone(),
                channel: "gui".into(),
                key: String::new(),
            }),
            body,
            priority: JobPriority::Normal,
            scope_summary: "sample".into(),
            correlation_id: correlation_id.clone(),
            sample: true,
        };
        match self.jobs.submit(submission)? {
            SubmitResult::Created(job_ref) => Ok(Launch::Loading {
                bundle_version: info.bundle_version,
                job_ref,
                report,
            }),
            // A new session reference never matches a running job's key.
            SubmitResult::Existing(_) => Err(RuntimeError::InvalidState),
        }
    }

    fn settle(&self, session: &mut Session, launch: Launch) {
        match launch {
            Launch::Loading {
                bundle_version,
                job_ref,
                report,
            } => {
                session.bundle_version = Some(bundle_version);
                session.job_ref = Some(job_ref.clone());
                session.report = Some(report);
                self.move_to(session, SampleSessionState::Preparing, None, Some(job_ref));
            }
            Launch::Unavailable(reason) => self.fail(session, reason, None),
        }
    }

    fn fail(&self, session: &mut Session, reason: &'static str, job_ref: Option<Id>) {
        session.discard_results();
        session.unavailable_reason = Some(reason);
        self.move_to(
            session,
            SampleSessionState::Unavailable,
            Some(reason),
            job_ref,
        );
    }

    /// Commits a transition and publishes its event.
    fn move_to(
        &self,
        session: &mut Session,
        to: SampleSessionState,
        reason: Option<&'static str>,
        job_ref: Option<Id>,
    ) {
        let previous = session.state;
        session.state = to;
        session.sequence += 1;
        if to != SampleSessionState::Unavailable {
            session.unavailable_reason = None;
        }
        // The clock cannot fail for any realistic time; the state is committed regardless.
        let Some(observed_at) = now() else {
            return;
        };
        self.sink.publish(
            SampleStateChanged {
                session_ref: session.session_ref.clone(),
                state: to.event_state(),
                previous_state: previous.event_state(),
                reason_key: reason.map(str::to_owned),
                job_ref: job_ref.clone(),
                sequence: session.sequence,
                observed_at,
            },
            job_ref,
        );
    }
}

fn open_session<'a>(
    slot: &'a mut Option<Session>,
    session_ref: &Id,
) -> Result<&'a mut Session, EngineError> {
    slot.as_mut()
        .filter(|session| {
            session.session_ref == *session_ref && session.state != SampleSessionState::Closed
        })
        .ok_or_else(session_closed)
}

fn session_closed() -> EngineError {
    EngineError::reason(SAMPLE_SESSION_CLOSED)
}

fn internal() -> EngineError {
    EngineError::reason("support.internal_error")
}

fn submit_error(error: RuntimeError) -> EngineError {
    match error {
        RuntimeError::QueueFull | RuntimeError::DeliveryBusy => {
            EngineError::reason("job.queue_full")
        }
        RuntimeError::ShuttingDown => EngineError::reason("job.shutting_down"),
        _ => internal(),
    }
}

fn new_id() -> Id {
    Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes())
}

fn now() -> Option<Timestamp> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    Timestamp::from_unix_millis(i64::try_from(elapsed.as_millis()).ok()?)
}
