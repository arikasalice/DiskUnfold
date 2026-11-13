use super::*;
use du_core_contracts::job::{JobFlow, JobFlowError};
use std::sync::mpsc;
type FlowBatch = (Vec<Box<dyn JobFlow>>, bool, Arc<dyn JobContext>);
struct MultiBody {
    sender: mpsc::Sender<FlowBatch>,
}
impl JobBody for MultiBody {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let mut flows = Vec::new();
        if let Ok(flow) = context.try_start_flow() {
            flows.push(flow);
        }
        if let Ok(flow) = context.try_start_flow() {
            flows.push(flow);
        }
        let third_busy = matches!(context.try_start_flow(), Err(JobFlowError::Busy));
        let _ = self.sender.send((flows, third_busy, context));
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
#[test]
fn slow_flow_does_not_block_other_result_or_allow_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let history = Arc::new(History::default());
    let events = Arc::new(Events::default());
    let runtime = Arc::new(JobRuntime::new_with_limits(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        history.clone(),
        events.clone(),
        FlowLimits {
            metadata_io: 2,
            queued_items: 4,
            item_bytes: 32,
        },
    )?);
    runtime.register(descriptor())?;
    let (sender, receiver) = mpsc::channel();
    let id = created(runtime.submit(sample_submission(Arc::new(MultiBody { sender }))?)?)?;
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    let (mut flows, busy, context) = receiver.recv_timeout(std::time::Duration::from_secs(2))?;
    assert!(busy);
    assert_eq!(flows.len(), 2);
    let fast = flows.pop().ok_or("missing fast flow")?;
    let slow = flows.pop().ok_or("missing slow flow")?;
    let (release_slow, slow_return) = mpsc::channel();
    let (slow_started, slow_ready) = mpsc::channel();
    let blocked = std::thread::spawn(move || {
        let _ = slow_started.send(());
        let _ = slow_return.recv();
        slow.finish(JobCounts {
            skipped: 1,
            ..Default::default()
        });
    });
    slow_ready.recv_timeout(std::time::Duration::from_secs(2))?;
    fast.report_item(b"fast").map_err(|e| format!("{e:?}"))?;
    fast.finish(JobCounts {
        completed: 1,
        ..Default::default()
    });
    assert_eq!(
        runtime.cancel(&id)?,
        CancelResult::Accepted(JobState::CancelRequested)
    );
    assert!(matches!(
        context.try_start_flow(),
        Err(JobFlowError::Cancelled)
    ));
    assert!(matches!(
        runtime.begin_shutdown(std::time::Duration::ZERO),
        Err(RuntimeError::ActiveJobs)
    ));
    runtime.flush()?;
    {
        let events = events.0.lock().map_err(|_| "events poisoned")?;
        assert!(events.iter().any(
            |event| matches!(event, JobEvent::ItemResult { payload, .. } if payload == b"fast")
        ));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, JobEvent::Finished { .. }))
        );
    }
    assert_eq!(runtime.state(&id)?, JobState::CancelRequested);
    release_slow.send(())?;
    blocked.join().map_err(|_| "blocked flow panicked")?;
    worker.join().map_err(|_| "worker panicked")??;
    runtime.flush()?;
    let events = events.0.lock().map_err(|_| "events poisoned")?;
    assert!(
        matches!(events.last(), Some(JobEvent::Finished { summary, .. }) if summary.state == JobState::Cancelled && summary.counts.completed == 1 && summary.counts.skipped == 1)
    );
    assert_eq!(history.calls.load(Ordering::SeqCst), 1);
    Ok(())
}
#[test]
fn a_job_without_the_metadata_class_cannot_open_metadata_flows()
-> Result<(), Box<dyn std::error::Error>> {
    verify_metadata_is_unsupported(2, ResourceClass::Compute)
}

#[test]
fn disabled_metadata_flows_are_unsupported_instead_of_busy()
-> Result<(), Box<dyn std::error::Error>> {
    verify_metadata_is_unsupported(0, ResourceClass::MetadataIo)
}

fn verify_metadata_is_unsupported(
    limit: usize,
    resource: ResourceClass,
) -> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let runtime = JobRuntime::new_with_limits(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
        FlowLimits {
            metadata_io: limit,
            queued_items: 4,
            item_bytes: 32,
        },
    )?;
    let mut descriptor = descriptor();
    descriptor.resource_classes = vec![resource];
    runtime.register(descriptor)?;
    let (sender, receiver) = mpsc::channel();
    let id = created(runtime.submit(sample_submission(Arc::new(MultiBody { sender }))?)?)?;
    assert_eq!(runtime.run_next()?, Some(id));
    let (flows, busy, _) = receiver.recv_timeout(std::time::Duration::from_secs(2))?;
    assert!(flows.is_empty());
    assert!(!busy);
    Ok(())
}

struct SingleBody {
    sender: mpsc::Sender<Box<dyn JobFlow>>,
}
impl JobBody for SingleBody {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        if let Ok(flow) = context.try_start_flow() {
            let _ = self.sender.send(flow);
        }
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
struct GatedSink {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    events: Mutex<Vec<JobEvent>>,
}
impl JobEvents for GatedSink {
    fn publish(&self, event: JobEvent) {
        if matches!(
            event,
            JobEvent::StateChanged {
                state: JobState::Running,
                ..
            }
        ) {
            self.entered.wait();
            self.release.wait();
        }
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }
}
#[test]
fn result_budget_and_size_are_bounded_before_copy() -> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let sink = Arc::new(GatedSink {
        entered: entered.clone(),
        release: release.clone(),
        events: Mutex::new(Vec::new()),
    });
    let runtime = Arc::new(JobRuntime::new_with_limits(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        sink.clone(),
        FlowLimits {
            metadata_io: 1,
            queued_items: 1,
            item_bytes: 4,
        },
    )?);
    runtime.register(descriptor())?;
    let (sender, receiver) = mpsc::channel();
    created(runtime.submit(sample_submission(Arc::new(SingleBody { sender }))?)?)?;
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    entered.wait();
    let flow = receiver.recv_timeout(std::time::Duration::from_secs(2))?;
    assert_eq!(
        flow.report_item(b"large"),
        Err(JobFlowError::PayloadTooLarge)
    );
    assert_eq!(flow.report_item(b"one"), Ok(()));
    assert_eq!(flow.report_item(b"two"), Err(JobFlowError::Busy));
    flow.finish(JobCounts {
        completed: 1,
        ..Default::default()
    });
    worker.join().map_err(|_| "worker panicked")??;
    release.wait();
    runtime.flush()?;
    let events = sink.events.lock().map_err(|_| "events poisoned")?;
    assert!(matches!(events.last(), Some(JobEvent::Finished { .. })));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, JobEvent::ItemResult { .. }))
            .count(),
        1
    );
    Ok(())
}
#[test]
fn abandoned_flow_fails_job_instead_of_claiming_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let events = Arc::new(Events::default());
    let runtime = Arc::new(JobRuntime::new_with_limits(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        events.clone(),
        FlowLimits {
            metadata_io: 1,
            queued_items: 1,
            item_bytes: 4,
        },
    )?);
    runtime.register(descriptor())?;
    let (sender, receiver) = mpsc::channel();
    let id = created(runtime.submit(sample_submission(Arc::new(SingleBody { sender }))?)?)?;
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    let flow = receiver.recv_timeout(std::time::Duration::from_secs(2))?;
    drop(flow);
    worker.join().map_err(|_| "worker panicked")??;
    runtime.flush()?;
    assert_eq!(runtime.state(&id)?, JobState::Failed);
    let events = events.0.lock().map_err(|_| "events poisoned")?;
    assert!(
        matches!(events.last(), Some(JobEvent::Finished { summary, .. }) if summary.failure_reason_key.as_deref() == Some("support.internal_error"))
    );
    Ok(())
}
