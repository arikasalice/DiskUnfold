//! F24 failure and current-result admission against the real model cache.
use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    job::{
        HistoryAppend, HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobHistory,
        JobOutcome, JobState, JobSummary, ProgressUnit, ResourceClass,
    },
    result::{ResultCompleteness, ResultVersionRead},
    visualization::rows::VizRowsRequest,
};
use du_engine_api::viz_pages::{ModelInput, ModelPages};
use du_job_runtime::{
    JobClock, JobEvent, JobEvents, JobIdentity, JobPriority, JobRuntime, Submission,
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}
struct Ids(AtomicU8);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        id(self.0.fetch_add(1, Ordering::SeqCst))
    }
}
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
fn submission(kind: &str) -> Submission {
    Submission {
        kind_id: kind.into(),
        deduplication_key: None,
        body: Arc::new(Body),
        priority: JobPriority::Normal,
        scope_summary: "fixture".into(),
        correlation_id: id(5),
        sample: false,
    }
}
fn publish(
    sink: &IngestSink,
    scope: u8,
    revision: u8,
) -> Result<Arc<du_analysis_index::ResultVersion>, String> {
    let mut build = sink.open(id(scope)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(revision), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn rejected_submission_and_stale_job_do_not_evict_current_model() -> Result<(), String> {
    let sink = IngestSink::default();
    let old = publish(&sink, 1, 2)?;
    let current = publish(&sink, 3, 4)?;
    let pages = ModelPages::new(
        sink.clone(),
        NonZeroUsize::MIN,
        Arc::new(Ids(AtomicU8::new(20))),
    );
    let reference = pages
        .register(ModelInput {
            version: current.clone(),
            condition_fingerprint: "current".into(),
            rows: vec![],
            groups: Default::default(),
        })
        .map_err(|e| format!("{e:?}"))?;
    let runtime = JobRuntime::new(
        Arc::new(Ids(AtomicU8::new(40))),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events),
    )
    .map_err(|e| format!("{e:?}"))?;
    runtime
        .register(JobDescriptor {
            kind_id: "viz.compute".into(),
            version: 1,
            display_name_key: "viz.compute".into(),
            controls: vec![JobControl::Cancel],
            safe_points: vec!["segment".into()],
            progress_unit: ProgressUnit::Items,
            resource_classes: vec![ResourceClass::Compute],
            history: HistoryMode::None,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        runtime
            .submit_viz_with_publication(submission("missing"), || Ok(()))
            .is_err()
    );
    let check_source = sink.clone();
    let submitted = runtime
        .submit_viz_with_publication(submission("viz.compute"), move || {
            check_source
                .with_current(&old, || ())
                .map_err(|_| "stale".to_owned())
        })
        .map_err(|e| format!("{e:?}"))?;
    let job = match submitted {
        du_job_runtime::SubmitResult::Created(id) => id,
        _ => return Err("unexpected deduplication".into()),
    };
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        runtime.state(&job).map_err(|e| format!("{e:?}"))?,
        JobState::Failed
    );
    let page = pages
        .rows(&VizRowsRequest {
            model_ref: reference,
            result_revision: current.version_id().clone(),
            start: 0,
            count: 1,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.result_revision, *current.version_id());
    Ok(())
}
