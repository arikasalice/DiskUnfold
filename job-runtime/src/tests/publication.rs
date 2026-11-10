use super::*;
use std::sync::atomic::AtomicBool;

mod duplicate_completion;
mod subscriptions;

struct QueryBody {
    kind: &'static str,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    proposed_result: Option<Id>,
}
impl JobBody for QueryBody {
    fn kind_id(&self) -> &str {
        self.kind
    }
    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        self.entered.wait();
        self.release.wait();
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: self.proposed_result.clone(),
        }
    }
}
fn setup() -> Result<(Arc<JobRuntime>, Id), Box<dyn std::error::Error>> {
    setup_kind("query.evaluate", Arc::new(Events::default()))
}

fn setup_kind(
    kind: &str,
    events: Arc<Events>,
) -> Result<(Arc<JobRuntime>, Id), Box<dyn std::error::Error>> {
    let id = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Identity(id.clone())),
        Arc::new(Clock),
        Arc::new(History::default()),
        events,
    )?);
    runtime.register(JobDescriptor {
        kind_id: kind.into(),
        version: 1,
        display_name_key: kind.into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::Compute],
        history: HistoryMode::None,
    })?;
    Ok((runtime, id))
}
fn submission(entered: Arc<Barrier>, release: Arc<Barrier>, correlation: Id) -> Submission {
    controlled_submission("query.evaluate", entered, release, correlation)
}

fn controlled_submission(
    kind: &'static str,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    correlation: Id,
) -> Submission {
    Submission {
        kind_id: kind.into(),
        deduplication_key: None,
        body: Arc::new(QueryBody {
            kind,
            entered,
            release,
            proposed_result: (kind == "viz.compute").then(|| correlation.clone()),
        }),
        priority: JobPriority::Normal,
        scope_summary: "query".into(),
        correlation_id: correlation,
        sample: false,
    }
}

#[test]
fn visualization_requires_its_own_publication_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup_kind("viz.compute", Arc::new(Events::default()))?;
    let job = controlled_submission(
        "viz.compute",
        Arc::new(Barrier::new(1)),
        Arc::new(Barrier::new(1)),
        id.clone(),
    );
    assert_eq!(
        runtime.submit(job.clone()),
        Err(RuntimeError::InvalidDescriptor)
    );
    assert_eq!(
        runtime.submit_with_publication(job, || Ok(id)),
        Err(RuntimeError::InvalidDescriptor)
    );
    Ok(())
}

#[test]
fn visualization_commit_is_private_and_cancelled_work_never_commits()
-> Result<(), Box<dyn std::error::Error>> {
    for cancel in [false, true] {
        let events = Arc::new(Events::default());
        let (runtime, id) = setup_kind("viz.compute", events.clone())?;
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let commits = Arc::new(AtomicUsize::new(0));
        let counter = commits.clone();
        let mut submission =
            controlled_submission("viz.compute", entered.clone(), release.clone(), id.clone());
        submission.deduplication_key = Some(DeduplicationKey {
            scope: "pinned-result".into(),
            input: "fixed-visualization-condition".into(),
            channel: "gui".into(),
            key: "strategy-revision".into(),
        });
        let result = runtime.submit_viz_with_publication(submission.clone(), move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })?;
        assert_eq!(result, SubmitResult::Created(id.clone()));
        let counter = commits.clone();
        assert_eq!(
            runtime.submit_viz_with_admission(
                submission,
                move || {
                    counter.fetch_add(10, Ordering::SeqCst);
                    Ok(())
                },
                |_| Err(RuntimeError::InvalidState),
            )?,
            SubmitResult::Existing(id.clone())
        );
        let worker = {
            let runtime = runtime.clone();
            std::thread::spawn(move || runtime.run_next())
        };
        entered.wait();
        assert_eq!(commits.load(Ordering::SeqCst), 0);
        let expected = if cancel {
            assert_eq!(
                runtime.cancel(&id)?,
                CancelResult::Accepted(JobState::CancelRequested)
            );
            JobState::Cancelled
        } else {
            JobState::Done
        };
        release.wait();
        worker.join().map_err(|_| "worker panicked")??;
        runtime.flush()?;
        assert_eq!(commits.load(Ordering::SeqCst), usize::from(!cancel));
        let snapshot = runtime.snapshot(&id)?;
        assert_eq!(snapshot.state, expected);
        assert_eq!(snapshot.result_version_id, None);
        let events = events.0.lock().map_err(|_| "events poisoned")?;
        let finished = events
            .iter()
            .filter(|event| matches!(event, JobEvent::Finished { .. }))
            .count();
        assert_eq!(finished, 1);
        assert!(
            matches!(events.last(), Some(JobEvent::Finished { summary, history_recorded: false, .. })
            if summary.state == expected && summary.result_version_id.is_none())
        );
    }
    Ok(())
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
