//! Contract test for the query publication boundary and a real folder result set.
use du_analysis_index::{
    IngestSink, Observation,
    query::{
        self,
        page::{MultiFolderKey, PageError, ResultSets},
    },
};
use du_core_contracts::{
    Id,
    job::{
        Checkpoint, HistoryAppend, HistoryMode, JobBody, JobContext, JobControl, JobDescriptor,
        JobHistory, JobOutcome, JobState, JobSummary, ProgressUnit, ResourceClass,
    },
    result::ResultCompleteness,
};
use du_job_runtime::{
    CancelResult, JobClock, JobEvent, JobEvents, JobIdentity, JobPriority, JobRuntime, Submission,
    SubmitResult,
};
use std::sync::{Arc, Barrier};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
struct Identity(Id);
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        self.0.clone()
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
struct Body {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}
impl JobBody for Body {
    fn kind_id(&self) -> &str {
        "query.evaluate"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let cancelled = context.checkpoint() == Checkpoint::Cancel;
        self.entered.wait();
        self.release.wait();
        JobOutcome {
            counts: Default::default(),
            completed_scope: !cancelled,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
fn setup() -> Result<(Arc<ResultSets>, query::QueryResult, Id), String> {
    let store = IngestSink::default();
    let mut build = store.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = id(2)?;
    build
        .seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pinned = store.pin_current(&version).map_err(|e| format!("{e:?}"))?;
    let candidate = query::run_folder(
        &query::Registry::baseline(),
        pinned,
        None,
        &query::Condition::All(vec![]),
        query::Sort {
            id: "sort.path",
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok((Arc::new(ResultSets::new(store)), candidate, version))
}
fn runtime(job: Id) -> Result<Arc<JobRuntime>, String> {
    let runtime = Arc::new(
        JobRuntime::new(
            Arc::new(Identity(job)),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events),
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    runtime
        .register(JobDescriptor {
            kind_id: "query.evaluate".into(),
            version: 1,
            display_name_key: "query.evaluate".into(),
            controls: vec![JobControl::Cancel],
            safe_points: vec!["item".into()],
            progress_unit: ProgressUnit::Items,
            resource_classes: vec![ResourceClass::Compute],
            history: HistoryMode::None,
        })
        .map_err(|e| format!("{e:?}"))?;
    Ok(runtime)
}

fn folder_key(candidate: &query::QueryResult, version: &Id) -> MultiFolderKey {
    MultiFolderKey {
        version: version.clone(),
        folder: query::FolderLocation::Item(0),
        tree: false,
        condition: candidate.condition.clone(),
        sorts: candidate.sorts.clone(),
    }
}

#[test]
fn cancelled_query_does_not_register_its_folder_candidate() -> Result<(), String> {
    let (sets, candidate, version) = setup()?;
    let query_ref = id(3)?;
    let job = id(4)?;
    let runtime = runtime(job.clone())?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let target = sets.clone();
    let output = query_ref.clone();
    let key = folder_key(&candidate, &version);
    let result = runtime
        .submit_with_publication(
            Submission {
                kind_id: "query.evaluate".into(),
                deduplication_key: None,
                body: Arc::new(Body {
                    entered: entered.clone(),
                    release: release.clone(),
                }),
                priority: JobPriority::Normal,
                scope_summary: "folder".into(),
                correlation_id: id(5)?,
                sample: false,
            },
            move || {
                target
                    .register_multi_folder(key, output, candidate)
                    .map_err(|e| format!("{e:?}"))
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result, SubmitResult::Created(job.clone()));
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    assert_eq!(
        runtime.cancel(&job),
        Ok(CancelResult::Accepted(JobState::CancelRequested))
    );
    release.wait();
    worker
        .join()
        .map_err(|_| "worker panicked")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(runtime.state(&job), Ok(JobState::Cancelled));
    assert!(matches!(
        sets.open_page(&query_ref, &version, 0, Some(1)),
        Err(PageError::StaleVersion)
    ));
    Ok(())
}

#[test]
fn completed_query_publishes_its_folder_candidate_before_terminal_result() -> Result<(), String> {
    let (sets, candidate, version) = setup()?;
    let query_ref = id(3)?;
    let job = id(4)?;
    let runtime = runtime(job.clone())?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let target = sets.clone();
    let output = query_ref.clone();
    let key = folder_key(&candidate, &version);
    runtime
        .submit_with_publication(
            Submission {
                kind_id: "query.evaluate".into(),
                deduplication_key: None,
                body: Arc::new(Body {
                    entered: entered.clone(),
                    release: release.clone(),
                }),
                priority: JobPriority::Normal,
                scope_summary: "folder".into(),
                correlation_id: id(5)?,
                sample: false,
            },
            move || {
                target
                    .register_multi_folder(key, output.clone(), candidate)
                    .map_err(|e| format!("{e:?}"))
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    release.wait();
    worker
        .join()
        .map_err(|_| "worker panicked")?
        .map_err(|e| format!("{e:?}"))?;
    let snapshot = runtime.snapshot(&job).map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.state, JobState::Done);
    assert_eq!(snapshot.result_version_id, Some(query_ref.clone()));
    let page = sets
        .open_page(&query_ref, &version, 0, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.items().len(), 1);
    Ok(())
}
