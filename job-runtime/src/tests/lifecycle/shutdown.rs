use super::*;
use std::sync::{
    Barrier,
    atomic::{AtomicBool, AtomicUsize},
    mpsc,
};

#[test]
fn shutdown_barrier_fails_if_required_history_did_not_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let history = Arc::new(History {
        fail: true,
        ..Default::default()
    });
    let runtime = JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        history.clone(),
        Arc::new(Events::default()),
    )?;
    runtime.register(descriptor())?;
    let id = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    assert_eq!(
        runtime.cancel(&id)?,
        CancelResult::Accepted(JobState::Cancelled)
    );

    let ticket = runtime.begin_shutdown(std::time::Duration::from_secs(1))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match ticket.poll() {
            ShutdownStatus::Failed => break,
            ShutdownStatus::Pending | ShutdownStatus::TimedOut => {
                if std::time::Instant::now() >= deadline {
                    return Err("failed history did not reach the shutdown barrier".into());
                }
                std::thread::yield_now();
            }
            ShutdownStatus::Complete => return Err("failed history was reported complete".into()),
        }
    }
    assert_eq!(ticket.history_failures(), Some(1));
    assert_eq!(ticket.poll(), ShutdownStatus::Failed);
    assert_eq!(ticket.history_failures(), Some(1));
    assert_eq!(history.calls.load(Ordering::SeqCst), 1);
    let terminal = runtime
        .snapshot(&id)?
        .terminal
        .ok_or("terminal result was not delivered before the barrier")?;
    assert!(!terminal.history_recorded);
    Ok(())
}

#[test]
fn shutdown_cancels_queued_work_and_closes_admission_before_terminal_history()
-> Result<(), Box<dyn std::error::Error>> {
    let history = Arc::new(History::default());
    let runtime = JobRuntime::new(
        Arc::new(Identity(Id::parse("00000000-0000-7000-8000-000000000001")?)),
        Arc::new(Clock),
        history.clone(),
        Arc::new(Events::default()),
    )?;
    let mut kind = descriptor();
    kind.resource_classes = vec![ResourceClass::FileChange];
    runtime.register(kind)?;
    let id = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    assert_eq!(
        runtime.close_admission_for_shutdown(false)?,
        AdmissionClose::Accepted {
            active_change_jobs: 0
        }
    );

    let cancelled = runtime.cancel_all_for_shutdown()?;

    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0].0, id);
    assert_eq!(
        cancelled[0].1,
        Ok((
            CancelResult::Accepted(JobState::Cancelled),
            runtime.snapshot(&id)?.sequence,
        ))
    );
    assert_eq!(
        runtime.active_count_for_resource_class(ResourceClass::FileChange)?,
        0
    );
    assert_eq!(runtime.run_next()?, None);
    assert_eq!(
        runtime.submit(sample_submission(Arc::new(InstantBody))?),
        Err(RuntimeError::ShuttingDown)
    );
    runtime.flush()?;
    assert_eq!(history.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime
            .snapshot(&id)?
            .terminal
            .map(|terminal| terminal.summary.state),
        Some(JobState::Cancelled)
    );
    Ok(())
}

#[test]
fn shutdown_cancellation_waits_for_running_body_then_preserves_cancelled_history()
-> Result<(), Box<dyn std::error::Error>> {
    let history = Arc::new(History::default());
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Identity(Id::parse("00000000-0000-7000-8000-000000000001")?)),
        Arc::new(Clock),
        history.clone(),
        Arc::new(Events::default()),
    )?);
    runtime.register(descriptor())?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let id = created(runtime.submit(sample_submission(Arc::new(Body {
        entered: entered.clone(),
        leave: release.clone(),
    }))?)?)?;
    assert_eq!(
        runtime.close_admission_for_shutdown(false)?,
        AdmissionClose::Accepted {
            active_change_jobs: 0
        }
    );
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    entered.wait();

    let cancelled = runtime.cancel_all_for_shutdown()?;

    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0].0, id);
    assert_eq!(
        cancelled[0].1.as_ref().map(|(result, _)| result),
        Ok(&CancelResult::Accepted(JobState::CancelRequested))
    );
    assert_eq!(
        runtime.active_count_for_resource_class(ResourceClass::MetadataIo)?,
        1
    );
    assert_eq!(history.calls.load(Ordering::SeqCst), 0);
    release.wait();
    assert_eq!(
        worker.join().map_err(|_| "worker panicked")??,
        Some(id.clone())
    );
    runtime.flush()?;
    assert_eq!(history.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime.active_count_for_resource_class(ResourceClass::MetadataIo)?,
        0
    );
    let terminal = runtime
        .snapshot(&id)?
        .terminal
        .ok_or("missing terminal result")?;
    assert_eq!(terminal.summary.state, JobState::Cancelled);
    assert!(terminal.history_recorded);
    Ok(())
}

#[test]
fn shutdown_cancellation_requires_an_admission_cut_and_change_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = JobRuntime::new(
        Arc::new(Identity(Id::parse("00000000-0000-7000-8000-000000000001")?)),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
    )?;
    let mut kind = descriptor();
    kind.kind_id = "change.apply".into();
    kind.resource_classes = vec![ResourceClass::FileChange];
    runtime.register(kind)?;
    let id = created(runtime.submit(named_submission("change.apply")?)?)?;

    assert_eq!(
        runtime.cancel_all_for_shutdown(),
        Err(RuntimeError::InvalidState)
    );
    assert_eq!(runtime.state(&id)?, JobState::Queued);
    assert_eq!(
        runtime.close_admission_for_shutdown(false)?,
        AdmissionClose::ConfirmRequired {
            active_change_jobs: 1
        }
    );
    assert_eq!(
        runtime.cancel_all_for_shutdown(),
        Err(RuntimeError::InvalidState)
    );
    assert_eq!(runtime.state(&id)?, JobState::Queued);
    assert_eq!(
        runtime.close_admission_for_shutdown(true)?,
        AdmissionClose::Accepted {
            active_change_jobs: 1
        }
    );
    assert_eq!(
        runtime.cancel_all_for_shutdown()?[0].1,
        Ok((
            CancelResult::Accepted(JobState::Cancelled),
            runtime.snapshot(&id)?.sequence,
        ))
    );
    runtime.flush()?;
    assert_eq!(runtime.state(&id)?, JobState::Cancelled);
    Ok(())
}

struct NamedBody(&'static str);

impl JobBody for NamedBody {
    fn kind_id(&self) -> &str {
        self.0
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

fn named_submission(kind: &'static str) -> Result<Submission, String> {
    let mut submission = sample_submission(Arc::new(NamedBody(kind)))?;
    submission.kind_id = kind.to_owned();
    Ok(submission)
}

#[test]
fn shutdown_confirmation_uses_change_kind_and_declines_without_closing_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let runtime = JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
    )?;
    let mut scan = descriptor();
    scan.resource_classes = vec![ResourceClass::FileChange];
    runtime.register(scan)?;
    let scan_id = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    assert_eq!(
        runtime.close_admission_for_shutdown(false)?,
        AdmissionClose::Accepted {
            active_change_jobs: 0
        }
    );
    assert_eq!(runtime.state(&scan_id)?, JobState::Queued);

    let change_runtime = JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(10),
            fallback: Id::parse("00000000-0000-7000-8000-000000000001")?,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
    )?;
    let mut change = descriptor();
    change.kind_id = "change.apply".into();
    change.resource_classes = vec![ResourceClass::Compute];
    change_runtime.register(change)?;
    let initial = created(change_runtime.submit(named_submission("change.apply")?)?)?;
    assert_eq!(
        change_runtime.close_admission_for_shutdown(false)?,
        AdmissionClose::ConfirmRequired {
            active_change_jobs: 1
        }
    );
    assert_eq!(change_runtime.state(&initial)?, JobState::Queued);
    let admitted_before_confirmation = change_runtime.submit(named_submission("change.apply")?)?;
    assert!(matches!(
        admitted_before_confirmation,
        SubmitResult::Created(_)
    ));
    assert_eq!(
        change_runtime.close_admission_for_shutdown(true)?,
        AdmissionClose::Accepted {
            active_change_jobs: 2
        }
    );
    assert_eq!(
        change_runtime.close_admission_for_shutdown(true)?,
        AdmissionClose::AlreadyClosed
    );
    assert_eq!(
        change_runtime.submit(named_submission("change.apply")?),
        Err(RuntimeError::ShuttingDown)
    );
    Ok(())
}

#[test]
fn accepted_shutdown_closes_admission_at_a_serialized_submission_cut()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
    )?);
    let mut kind = descriptor();
    kind.kind_id = "change.apply".into();
    kind.resource_classes = vec![ResourceClass::Compute];
    runtime.register(kind)?;

    let start = Arc::new(Barrier::new(5));
    let stop = Arc::new(AtomicBool::new(false));
    let submitted = Arc::new(AtomicUsize::new(0));
    let (ready, ready_rx) = mpsc::sync_channel(1);
    let submission = named_submission("change.apply")?;
    let mut workers = Vec::new();
    for _ in 0..4 {
        let runtime = runtime.clone();
        let start = start.clone();
        let stop = stop.clone();
        let submitted = submitted.clone();
        let ready = ready.clone();
        let submission = submission.clone();
        workers.push(std::thread::spawn(move || {
            start.wait();
            let mut signaled = false;
            while !stop.load(Ordering::Acquire) {
                if runtime.submit(submission.clone()).is_ok() {
                    submitted.fetch_add(1, Ordering::AcqRel);
                    if !signaled {
                        let _ = ready.try_send(());
                        signaled = true;
                    }
                } else {
                    std::thread::yield_now();
                }
            }
        }));
    }
    start.wait();
    ready_rx.recv_timeout(std::time::Duration::from_secs(2))?;

    assert!(matches!(
        runtime.close_admission_for_shutdown(true)?,
        AdmissionClose::Accepted {
            active_change_jobs: 1..=64
        }
    ));
    stop.store(true, Ordering::Release);
    for worker in workers {
        worker.join().map_err(|_| "submission worker panicked")?;
    }
    assert_eq!(runtime.list(None)?.len(), submitted.load(Ordering::Acquire));
    assert_eq!(runtime.submit(submission), Err(RuntimeError::ShuttingDown));
    Ok(())
}
