//! The sample flow over a real job runtime with fake bundle bodies.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use du_analysis_index::SampleSessionKey;
use du_core_contracts::{
    Id,
    job::{
        HistoryAppend, JobBody, JobContext, JobCounts, JobHistory, JobOutcome, JobState, JobSummary,
    },
    sample_ipc::{SampleSession, SampleSessionState},
};
use du_job_runtime::{FlowLimits, JobClock, JobEvent, JobEvents, JobIdentity, JobRuntime};

use super::{Body, Report, job_id};
use crate::{
    api::EngineBuilder,
    sample::{
        KIND, SampleBundleInfo, SampleEvents, SampleLoadReport, SampleLoader, SampleSessions,
        register,
    },
    workers::Workers,
};

struct Ids(AtomicUsize);

impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        job_id(10_000 + self.0.fetch_add(1, Ordering::Relaxed) as u64)
    }
}

struct Clock;

impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        1_000
    }
}

#[derive(Default)]
struct History(AtomicUsize);

impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(HistoryAppend::Added)
    }
}

struct Downstream(Mutex<Vec<JobState>>);

impl JobEvents for Downstream {
    fn publish(&self, event: JobEvent) {
        if let (JobEvent::Finished { summary, .. }, Ok(mut list)) = (event, self.0.lock()) {
            list.push(summary.state);
        }
    }
}

/// Runs until cancelled when `hold` is set, otherwise finishes at once.
struct Held {
    hold: bool,
}

impl JobBody for Held {
    fn kind_id(&self) -> &str {
        KIND
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let mut outcome = Body.run(context.clone());
        loop {
            if !self.hold {
                break;
            }
            if context.checkpoint() == du_core_contracts::job::Checkpoint::Cancel {
                outcome.completed_scope = false;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        outcome
    }
}

struct Loader {
    hold: bool,
    report: Arc<Report>,
}

impl SampleLoader for Loader {
    fn open(&self) -> Result<SampleBundleInfo, &'static str> {
        Ok(SampleBundleInfo {
            bundle_version: "b1".into(),
        })
    }

    fn job(&self, _: &SampleSessionKey) -> (Arc<dyn JobBody>, Arc<dyn SampleLoadReport>) {
        (Arc::new(Held { hold: self.hold }), self.report.clone())
    }

    fn release(&self, _: &SampleSessionKey) -> u64 {
        0
    }
}

struct Real {
    sessions: Arc<SampleSessions>,
    runtime: Arc<JobRuntime>,
    history: Arc<History>,
    downstream: Arc<Downstream>,
}

fn real(hold: bool) -> Result<Real, String> {
    let history = Arc::new(History::default());
    let downstream = Arc::new(Downstream(Mutex::default()));
    let events = Arc::new(SampleEvents::new(downstream.clone()));
    let runtime = Arc::new(
        JobRuntime::new_with_limits(
            Arc::new(Ids(AtomicUsize::new(0))),
            Arc::new(Clock),
            history.clone(),
            events.clone(),
            FlowLimits::disabled(),
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    let workers = Arc::new(Workers::start(runtime.clone()).map_err(|e| e.to_string())?);
    let report = Arc::new(Report::default());
    let sink = Arc::new(super::Sink::default());
    let mut builder = EngineBuilder::default();
    let loader = Arc::new(Loader { hold, report });
    let sessions = register(&mut builder, workers, loader, sink).map_err(|e| format!("{e:?}"))?;
    events.bind(sessions.clone())?;
    Ok(Real {
        sessions,
        runtime,
        history,
        downstream,
    })
}

fn wait_for(mut done: impl FnMut() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        if Instant::now() > deadline {
            return Err("timed out".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn status(real: &Real, session: &Id) -> Result<SampleSession, String> {
    super::ok(real.sessions.status(session))
}

#[test]
fn a_finished_load_job_activates_the_session_without_history() -> Result<(), String> {
    let real = real(false)?;
    let opened = super::ok(real.sessions.open(&job_id(1)))?;
    wait_for(|| {
        status(&real, &opened.session_ref).is_ok_and(|v| v.state == SampleSessionState::Active)
    })?;
    // The terminal event also reaches the downstream receiver.
    wait_for(|| real.downstream.0.lock().is_ok_and(|list| list.len() == 1))?;
    assert_eq!(real.history.0.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn closing_a_running_load_cancels_the_real_job() -> Result<(), String> {
    let real = real(true)?;
    let opened = super::ok(real.sessions.open(&job_id(1)))?;
    let job = opened.job_ref.clone().ok_or("no job")?;
    wait_for(|| {
        real.runtime
            .state(&job)
            .is_ok_and(|s| s == JobState::Running)
    })?;
    super::ok(real.sessions.close(&opened.session_ref))?;
    wait_for(|| {
        real.runtime
            .state(&job)
            .is_ok_and(|s| s == JobState::Cancelled)
    })?;
    assert!(status(&real, &opened.session_ref).is_err());
    assert_eq!(real.history.0.load(Ordering::Relaxed), 0);
    let _ = JobCounts::default();
    Ok(())
}
