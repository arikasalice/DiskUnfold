use super::*;
use du_core_contracts::job::{
    HistoryAppend, HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobHistory,
    JobOutcome, JobSummary, ProgressUnit, ResourceClass,
};
use du_job_runtime::{
    JobClock, JobEvent, JobEvents, JobPriority, JobRuntime, Submission, SubmitResult,
};
use std::sync::Mutex;

struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        0
    }
}
struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}
struct Events;
impl JobEvents for Events {
    fn publish(&self, _: JobEvent) {}
}
struct Body;
impl JobBody for Body {
    fn kind_id(&self) -> &str {
        "viz.compute"
    }
    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        JobOutcome {
            counts: Default::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

pub(super) fn publish(
    session: Arc<Session>,
    candidate: du_engine_api::viz_pages::ranked_group::DeferredRankedGroup,
) -> Result<String, String> {
    execute(
        session,
        candidate,
        false,
        du_core_contracts::job::JobState::Done,
    )
}

pub(super) fn execute(
    session: Arc<Session>,
    candidate: du_engine_api::viz_pages::ranked_group::DeferredRankedGroup,
    cancel: bool,
    expected: du_core_contracts::job::JobState,
) -> Result<String, String> {
    let runtime = JobRuntime::new(
        Arc::new(Ids::default()),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events),
    )
    .map_err(|e| format!("{e:?}"))?;
    runtime
        .register(JobDescriptor {
            kind_id: "viz.compute".into(),
            version: 1,
            display_name_key: "job.viz.compute".into(),
            controls: vec![JobControl::Cancel],
            safe_points: vec!["path_range_end".into()],
            progress_unit: ProgressUnit::Items,
            resource_classes: vec![ResourceClass::Compute],
            history: HistoryMode::None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let candidate = Arc::new(Mutex::new(candidate));
    let reference = candidate
        .lock()
        .map_err(|_| "lock")?
        .reserved_reference()
        .to_owned();
    let commit = candidate.clone();
    let submitted = runtime
        .submit_viz_with_retained_admission(
            Submission {
                kind_id: "viz.compute".into(),
                deduplication_key: None,
                body: Arc::new(Body),
                priority: JobPriority::Normal,
                scope_summary: "fixture".into(),
                correlation_id: Id::from_v7_bytes([8; 16]),
                sample: false,
            },
            candidate,
            move || {
                let mut candidate = commit.lock().map_err(|_| "lock".to_owned())?;
                session
                    .commit_ranked_group(&mut candidate, &mut || false)
                    .map(|_| ())
                    .map_err(|e| format!("{e:?}"))
            },
            |_| Ok(()),
        )
        .map_err(|e| format!("{e:?}"))?;
    let SubmitResult::Created(job) = submitted else {
        return Err("unexpected reuse".into());
    };
    if cancel {
        runtime.cancel(&job).map_err(|e| format!("{e:?}"))?;
    }
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    assert_eq!(runtime.state(&job).map_err(|e| format!("{e:?}"))?, expected);
    Ok(reference)
}
