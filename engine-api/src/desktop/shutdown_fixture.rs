//! A real job runtime with one running `change.apply` job, for host shutdown tests.
//!
//! The fixture composes the same runtime, workers, job commands and shutdown
//! hooks as the desktop composition. Only the job body is a stand-in: it runs
//! until it observes cancellation, or, when it models a stuck OS call, until the
//! test releases it.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use du_core_contracts::{
    Id,
    job::{
        Checkpoint, HistoryAppend, HistoryMode, JobBody, JobContext, JobControl, JobDescriptor,
        JobHistory, JobOutcome, JobState, JobSummary, ProgressUnit, ResourceClass,
    },
};
use du_job_runtime::{
    JobClock, JobEvent, JobEvents, JobIdentity, JobPriority, JobRuntime, Submission, SubmitResult,
};

use crate::{
    api::{Engine, EngineBuilder},
    workers::Workers,
};

const KIND: &str = "change.apply";

struct Identity(AtomicU64);

impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        let next = self.0.fetch_add(1, Ordering::SeqCst);
        let mut bytes = [0_u8; 16];
        bytes[8..].copy_from_slice(&next.to_be_bytes());
        Id::from_v7_bytes(bytes)
    }
}

struct Clock;

impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        1_700_000_000_000
    }
}

struct NoEvents;

impl JobEvents for NoEvents {
    fn publish(&self, _: JobEvent) {}
}

#[derive(Default)]
struct RecordedHistory(Mutex<Vec<JobState>>);

impl JobHistory for RecordedHistory {
    fn append(&self, summary: &JobSummary) -> Result<HistoryAppend, String> {
        self.0
            .lock()
            .map_err(|_| "history lock is poisoned".to_owned())?
            .push(summary.state);
        Ok(HistoryAppend::Added)
    }
}

struct Body {
    started: Arc<AtomicBool>,
    released: Arc<AtomicBool>,
    ignores_cancel: bool,
}

impl JobBody for Body {
    fn kind_id(&self) -> &str {
        KIND
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        self.started.store(true, Ordering::SeqCst);
        let mut cancelled = false;
        while !self.released.load(Ordering::SeqCst) {
            if !self.ignores_cancel && context.checkpoint() == Checkpoint::Cancel {
                cancelled = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        JobOutcome {
            counts: Default::default(),
            completed_scope: !cancelled,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

/// A composed engine whose job runtime holds one running `change.apply` job.
pub struct RunningChangeJob {
    /// Engine with the job commands and the production shutdown hooks installed.
    pub engine: Arc<Engine>,
    /// Reference of the running change job.
    pub job_ref: Id,
    released: Arc<AtomicBool>,
    history: Arc<RecordedHistory>,
    retired: Arc<AtomicUsize>,
}

impl RunningChangeJob {
    /// Starts the job and waits until its body is running.
    ///
    /// With `stuck_in_os_call` the body ignores cancellation until
    /// [`RunningChangeJob::release`], as a body blocked inside an OS call would.
    pub fn start(stuck_in_os_call: bool) -> Result<Self, String> {
        let history = Arc::new(RecordedHistory::default());
        let runtime = Arc::new(
            JobRuntime::new(
                Arc::new(Identity(AtomicU64::new(1))),
                Arc::new(Clock),
                history.clone(),
                Arc::new(NoEvents),
            )
            .map_err(|error| error.to_string())?,
        );
        runtime
            .register(JobDescriptor {
                kind_id: KIND.into(),
                version: 1,
                display_name_key: KIND.into(),
                controls: vec![JobControl::Cancel],
                safe_points: vec!["item".into()],
                progress_unit: ProgressUnit::Items,
                resource_classes: vec![ResourceClass::FileChange],
                history: HistoryMode::Summary,
            })
            .map_err(|error| error.to_string())?;
        let workers = Arc::new(Workers::start(runtime.clone()).map_err(|error| error.to_string())?);
        let mut builder = EngineBuilder::default();
        crate::jobs::register(&mut builder, runtime.clone());
        builder.on_shutdown_hooks(super::shutdown::hooks(runtime.clone(), workers.clone()));
        let retired = Arc::new(AtomicUsize::new(0));
        let retire_count = retired.clone();
        builder.on_session_retire(move || {
            retire_count.fetch_add(1, Ordering::SeqCst);
        });
        let engine = Arc::new(builder.build().map_err(|error| error.to_string())?);

        let started = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let correlation = Id::from_v7_bytes([9; 16]);
        let job_ref = match runtime
            .submit(Submission {
                kind_id: KIND.into(),
                deduplication_key: None,
                body: Arc::new(Body {
                    started: started.clone(),
                    released: released.clone(),
                    ignores_cancel: stuck_in_os_call,
                }),
                priority: JobPriority::Normal,
                scope_summary: "shutdown fixture".into(),
                correlation_id: correlation,
                sample: false,
            })
            .map_err(|error| error.to_string())?
        {
            SubmitResult::Created(id) | SubmitResult::Existing(id) => id,
        };
        workers.notify();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !started.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                return Err("change job did not start".to_owned());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(Self {
            engine,
            job_ref,
            released,
            history,
            retired,
        })
    }

    /// Lets a body that is stuck in a modeled OS call return.
    pub fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
    }

    /// How many times the engine retired its window session.
    pub fn window_sessions_retired(&self) -> usize {
        self.retired.load(Ordering::SeqCst)
    }

    /// Terminal states appended to job history so far.
    pub fn recorded_history(&self) -> Vec<JobState> {
        self.history
            .0
            .lock()
            .map(|states| states.clone())
            .unwrap_or_default()
    }
}
