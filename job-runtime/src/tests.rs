use super::*;
use du_core_contracts::job::{JobCounts, ProgressUnit};
use std::sync::{Barrier, atomic::AtomicUsize};

struct Identity(Id);
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        self.0.clone()
    }
}
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        123
    }
}
#[derive(Default)]
struct History {
    calls: AtomicUsize,
    fail: bool,
}
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err("db.busy".into())
        } else {
            Ok(HistoryAppend::Added)
        }
    }
}
#[derive(Default)]
struct Events(Mutex<Vec<JobEvent>>);
impl JobEvents for Events {
    fn publish(&self, event: JobEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}
struct Body {
    entered: Arc<Barrier>,
    leave: Arc<Barrier>,
}
impl JobBody for Body {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        self.entered.wait();
        self.leave.wait();
        let cancelled = context.checkpoint() == Checkpoint::Cancel;
        JobOutcome {
            counts: JobCounts {
                completed: 1,
                ..Default::default()
            },
            completed_scope: !cancelled,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: "scan.metadata".into(),
        version: 1,
        display_name_key: "scan".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::MetadataIo],
        history: HistoryMode::Summary,
    }
}
#[test]
fn cancellation_waits_for_body_and_history_failure_keeps_terminal_event()
-> Result<(), Box<dyn std::error::Error>> {
    let history = Arc::new(History {
        fail: true,
        ..Default::default()
    });
    let events = Arc::new(Events::default());
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Identity(Id::parse("00000000-0000-7000-8000-000000000001")?)),
        Arc::new(Clock),
        history.clone(),
        events.clone(),
    )?);
    runtime.register(descriptor())?;
    assert_eq!(
        runtime.register(descriptor()),
        Err(RuntimeError::DuplicateKind)
    );
    let entered = Arc::new(Barrier::new(2));
    let leave = Arc::new(Barrier::new(2));
    let submission = Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: Some(DeduplicationKey {
            scope: "scope".into(),
            input: "input".into(),
            channel: "gui".into(),
            key: "key".into(),
        }),
        body: Arc::new(Body {
            entered: entered.clone(),
            leave: leave.clone(),
        }),
        priority: JobPriority::Normal,
        scope_summary: "summary".into(),
        correlation_id: Id::parse("00000000-0000-7000-8000-000000000999")?,
        sample: false,
    };
    let mut missing = submission.clone();
    missing.kind_id = "missing".into();
    assert_eq!(runtime.submit(missing), Err(RuntimeError::KindUnknown));
    let id = match runtime.submit(submission.clone())? {
        SubmitResult::Created(id) => id,
        _ => return Err("expected created".into()),
    };
    assert_eq!(
        runtime.submit(submission)?,
        SubmitResult::Existing(id.clone())
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
    assert_eq!(
        runtime.cancel(&id)?,
        CancelResult::NoEffect(JobState::CancelRequested)
    );
    assert_eq!(runtime.state(&id)?, JobState::CancelRequested);
    leave.wait();
    worker.join().map_err(|_| "worker panicked")??;
    assert_eq!(runtime.state(&id)?, JobState::Cancelled);
    runtime.flush()?;
    assert_eq!(history.calls.load(Ordering::SeqCst), 1);
    let events = events.0.lock().map_err(|_| "events poisoned")?;
    assert!(
        matches!(events.last(), Some(JobEvent::Finished { summary, history_recorded: false, warning: Some(_), .. }) if summary.state == JobState::Cancelled)
    );
    Ok(())
}

struct SequentialIdentity {
    next: AtomicUsize,
    fallback: Id,
}
impl JobIdentity for SequentialIdentity {
    fn next_id(&self) -> Id {
        let next = self.next.fetch_add(1, Ordering::SeqCst);
        Id::parse(&format!("00000000-0000-7000-8000-{next:012x}"))
            .unwrap_or_else(|_| self.fallback.clone())
    }
}
struct InstantBody;
impl JobBody for InstantBody {
    fn kind_id(&self) -> &str {
        "scan.metadata"
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
fn sample_submission(body: Arc<dyn JobBody>) -> Result<Submission, String> {
    Ok(Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body,
        priority: JobPriority::Normal,
        scope_summary: "summary".into(),
        correlation_id: Id::parse("00000000-0000-7000-8000-000000000999")?,
        sample: false,
    })
}
fn created(result: SubmitResult) -> Result<Id, Box<dyn std::error::Error>> {
    match result {
        SubmitResult::Created(id) => Ok(id),
        SubmitResult::Existing(_) => Err("expected new job".into()),
    }
}
#[test]
fn priority_and_fifo_selection_and_queued_cancel() -> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let history = Arc::new(History::default());
    let events = Arc::new(Events::default());
    let runtime = JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        history.clone(),
        events.clone(),
    )?;
    runtime.register(descriptor())?;
    let base = sample_submission(Arc::new(InstantBody))?;
    let low = created(runtime.submit(base.clone())?)?;
    let mut high = base.clone();
    high.priority = JobPriority::High;
    let first = created(runtime.submit(high.clone())?)?;
    let second = created(runtime.submit(high)?)?;
    assert_eq!(
        runtime.cancel(&low)?,
        CancelResult::Accepted(JobState::Cancelled)
    );
    assert_eq!(runtime.run_next()?, Some(first.clone()));
    assert_eq!(runtime.run_next()?, Some(second));
    assert_eq!(runtime.run_next()?, None);
    runtime.flush()?;
    assert_eq!(history.calls.load(Ordering::SeqCst), 3);
    let events = events.0.lock().map_err(|_| "events poisoned")?;
    let first_events: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            JobEvent::StateChanged {
                job_id, sequence, ..
            } if *job_id == first => Some(*sequence),
            JobEvent::Finished {
                summary, sequence, ..
            } if summary.job_id == first => Some(*sequence),
            _ => None,
        })
        .collect();
    assert_eq!(first_events, vec![2, 3, 4]);
    Ok(())
}
#[test]
fn deduplication_separates_input_channel_and_missing_keys() -> Result<(), Box<dyn std::error::Error>>
{
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
    runtime.register(descriptor())?;
    let mut submission = sample_submission(Arc::new(InstantBody))?;
    submission.deduplication_key = Some(DeduplicationKey {
        scope: "scope".into(),
        input: "one".into(),
        channel: "gui".into(),
        key: "key".into(),
    });
    let first = created(runtime.submit(submission.clone())?)?;
    assert_eq!(
        runtime.submit(submission.clone())?,
        SubmitResult::Existing(first)
    );
    if let Some(key) = &mut submission.deduplication_key {
        key.input = "two".into();
    }
    created(runtime.submit(submission.clone())?)?;
    if let Some(key) = &mut submission.deduplication_key {
        key.channel = "cli".into();
    }
    created(runtime.submit(submission.clone())?)?;
    submission.deduplication_key = None;
    created(runtime.submit(submission.clone())?)?;
    created(runtime.submit(submission)?)?;
    Ok(())
}

#[test]
fn active_limit_keeps_fifth_queued() -> Result<(), Box<dyn std::error::Error>> {
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
    runtime.register(descriptor())?;
    let entered = Arc::new(Barrier::new(5));
    let leave = Arc::new(Barrier::new(5));
    let body = Arc::new(Body {
        entered: entered.clone(),
        leave: leave.clone(),
    });
    let mut workers = Vec::new();
    for _ in 0..4 {
        created(runtime.submit(sample_submission(body.clone())?)?)?;
        let worker_runtime = runtime.clone();
        workers.push(std::thread::spawn(move || worker_runtime.run_next()));
    }
    entered.wait();
    let fifth = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    assert_eq!(runtime.run_next()?, None);
    assert_eq!(runtime.state(&fifth)?, JobState::Queued);
    leave.wait();
    for worker in workers {
        worker.join().map_err(|_| "worker panicked")??;
    }
    assert_eq!(runtime.run_next()?, Some(fifth));
    Ok(())
}

#[test]
fn file_change_class_has_single_active_job() -> Result<(), Box<dyn std::error::Error>> {
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
    kind.resource_classes = vec![ResourceClass::FileChange];
    runtime.register(kind)?;
    let entered = Arc::new(Barrier::new(2));
    let leave = Arc::new(Barrier::new(2));
    let first = created(runtime.submit(sample_submission(Arc::new(Body {
        entered: entered.clone(),
        leave: leave.clone(),
    }))?)?)?;
    let second = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    entered.wait();
    assert_eq!(runtime.run_next()?, None);
    assert_eq!(runtime.state(&second)?, JobState::Queued);
    leave.wait();
    assert_eq!(worker.join().map_err(|_| "worker panicked")??, Some(first));
    assert_eq!(runtime.run_next()?, Some(second));
    Ok(())
}

mod blocked;
mod history_boundary;
mod history_receipt;
mod lifecycle;
mod listing;
mod multiflow;
mod progress;
mod publication;
mod race;

mod pause;
