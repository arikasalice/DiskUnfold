use super::*;
use std::sync::atomic::AtomicBool;

struct QueryBody {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}
impl JobBody for QueryBody {
    fn kind_id(&self) -> &str {
        "query.evaluate"
    }
    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        self.entered.wait();
        self.release.wait();
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
fn setup() -> Result<(Arc<JobRuntime>, Id), Box<dyn std::error::Error>> {
    let id = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Identity(id.clone())),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
    )?);
    runtime.register(JobDescriptor {
        kind_id: "query.evaluate".into(),
        version: 1,
        display_name_key: "query.evaluate".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::Compute],
        history: HistoryMode::None,
    })?;
    Ok((runtime, id))
}
fn submission(entered: Arc<Barrier>, release: Arc<Barrier>, correlation: Id) -> Submission {
    Submission {
        kind_id: "query.evaluate".into(),
        deduplication_key: None,
        body: Arc::new(QueryBody { entered, release }),
        priority: JobPriority::Normal,
        scope_summary: "query".into(),
        correlation_id: correlation,
        sample: false,
    }
}

#[test]
fn cancellation_before_commit_skips_publication() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let published = Arc::new(AtomicBool::new(false));
    let value = published.clone();
    let output = id.clone();
    assert_eq!(
        runtime.submit_with_publication(
            submission(entered.clone(), release.clone(), id.clone()),
            move || {
                value.store(true, Ordering::SeqCst);
                Ok(output)
            }
        )?,
        SubmitResult::Created(id.clone())
    );
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    assert_eq!(
        runtime.cancel(&id)?,
        CancelResult::Accepted(JobState::CancelRequested)
    );
    release.wait();
    worker.join().map_err(|_| "worker panicked")??;
    assert_eq!(runtime.state(&id)?, JobState::Cancelled);
    assert!(!published.load(Ordering::SeqCst));
    assert_eq!(runtime.snapshot(&id)?.result_version_id, None);
    Ok(())
}

#[test]
fn query_cannot_skip_the_publication_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    assert_eq!(
        runtime.submit(submission(entered, release, id)),
        Err(RuntimeError::InvalidDescriptor)
    );
    Ok(())
}

#[test]
fn commit_before_cancellation_finishes_with_registered_result()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let publish_entered = Arc::new(Barrier::new(2));
    let publish_release = Arc::new(Barrier::new(2));
    let p = publish_entered.clone();
    let q = publish_release.clone();
    let output = id.clone();
    runtime.submit_with_publication(
        submission(entered.clone(), release.clone(), id.clone()),
        move || {
            p.wait();
            q.wait();
            Ok(output)
        },
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    release.wait();
    publish_entered.wait();
    // The external transaction does not hold the global job table lock.
    assert_eq!(runtime.state(&id)?, JobState::Running);
    let cancel = {
        let runtime = runtime.clone();
        let id = id.clone();
        std::thread::spawn(move || runtime.cancel(&id))
    };
    publish_release.wait();
    worker.join().map_err(|_| "worker panicked")??;
    assert_eq!(
        cancel.join().map_err(|_| "cancel panicked")??,
        CancelResult::AlreadyTerminal(JobState::Done)
    );
    assert_eq!(runtime.snapshot(&id)?.result_version_id, Some(id));
    Ok(())
}

#[test]
fn failed_publication_reports_failure_without_result() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    runtime.submit_with_publication(
        submission(entered.clone(), release.clone(), id.clone()),
        || Err("result.capacity_exceeded".into()),
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    release.wait();
    worker.join().map_err(|_| "worker panicked")??;
    let snapshot = runtime.snapshot(&id)?;
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(
        snapshot.failure_reason_key.as_deref(),
        Some("result.capacity_exceeded")
    );
    assert_eq!(snapshot.result_version_id, None);
    Ok(())
}
