use super::*;

struct BlockingEvents {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    events: Mutex<Vec<JobEvent>>,
}
impl JobEvents for BlockingEvents {
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
fn cancellation_returns_while_event_sink_is_blocked() -> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let sink_entered = Arc::new(Barrier::new(2));
    let sink_release = Arc::new(Barrier::new(2));
    let events = Arc::new(BlockingEvents {
        entered: sink_entered.clone(),
        release: sink_release.clone(),
        events: Mutex::new(Vec::new()),
    });
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        events.clone(),
    )?);
    runtime.register(descriptor())?;
    let body_entered = Arc::new(Barrier::new(2));
    let body_release = Arc::new(Barrier::new(2));
    let id = created(runtime.submit(sample_submission(Arc::new(Body {
        entered: body_entered.clone(),
        leave: body_release.clone(),
    }))?)?)?;
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    sink_entered.wait();
    body_entered.wait();
    let (sender, receiver) = std::sync::mpsc::channel();
    let cancel_runtime = runtime.clone();
    let cancel_id = id.clone();
    let cancel_worker = std::thread::spawn(move || {
        let _ = sender.send(cancel_runtime.cancel(&cancel_id));
    });
    let result = receiver.recv_timeout(std::time::Duration::from_secs(2));
    sink_release.wait();
    body_release.wait();
    assert_eq!(result??, CancelResult::Accepted(JobState::CancelRequested));
    cancel_worker.join().map_err(|_| "cancel panicked")?;
    worker.join().map_err(|_| "worker panicked")??;
    runtime.flush()?;
    let events = events.events.lock().map_err(|_| "events poisoned")?;
    let sequences: Vec<_> = events
        .iter()
        .map(|event| match event {
            JobEvent::StateChanged { sequence, .. } | JobEvent::Finished { sequence, .. } => {
                *sequence
            }
        })
        .collect();
    assert_eq!(sequences, vec![2, 3, 4, 5]);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, JobEvent::Finished { .. }))
            .count(),
        1
    );
    Ok(())
}
pub(super) struct BlockingHistory {
    pub(super) entered: Arc<Barrier>,
    pub(super) release: Arc<Barrier>,
    pub(super) order: Arc<Mutex<Vec<&'static str>>>,
}
impl JobHistory for BlockingHistory {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        self.entered.wait();
        self.release.wait();
        if let Ok(mut order) = self.order.lock() {
            order.push("history");
        }
        Ok(HistoryAppend::Added)
    }
}
struct OrderedEvents(Arc<Mutex<Vec<&'static str>>>);
impl JobEvents for OrderedEvents {
    fn publish(&self, event: JobEvent) {
        if matches!(event, JobEvent::Finished { .. })
            && let Ok(mut order) = self.0.lock()
        {
            order.push("finished");
        }
    }
}
#[test]
fn queued_cancel_returns_during_blocked_history_and_finishes_in_order()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let order = Arc::new(Mutex::new(Vec::new()));
    let history = Arc::new(BlockingHistory {
        entered: entered.clone(),
        release: release.clone(),
        order: order.clone(),
    });
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        history,
        Arc::new(OrderedEvents(order.clone())),
    )?);
    runtime.register(descriptor())?;
    let id = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let cancel_runtime = runtime.clone();
    let cancel_id = id.clone();
    let worker = std::thread::spawn(move || {
        let _ = sender.send(cancel_runtime.cancel(&cancel_id));
    });
    entered.wait();
    let result = receiver.recv_timeout(std::time::Duration::from_secs(2));
    release.wait();
    assert_eq!(result??, CancelResult::Accepted(JobState::Cancelled));
    worker.join().map_err(|_| "worker panicked")?;
    runtime.flush()?;
    assert_eq!(
        *order.lock().map_err(|_| "order poisoned")?,
        vec!["history", "finished"]
    );
    Ok(())
}

#[test]
fn dropping_runtime_does_not_wait_for_pending_history() -> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let order = Arc::new(Mutex::new(Vec::new()));
    let history = Arc::new(BlockingHistory {
        entered: entered.clone(),
        release: release.clone(),
        order: order.clone(),
    });
    let runtime = JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        history,
        Arc::new(OrderedEvents(order.clone())),
    )?;
    runtime.register(descriptor())?;
    let id = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    runtime.cancel(&id)?;
    entered.wait();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        drop(runtime);
        let _ = sender.send(());
    });
    let early = receiver.recv_timeout(std::time::Duration::from_millis(100));
    release.wait();
    worker.join().map_err(|_| "drop panicked")?;
    assert!(early.is_ok());
    Ok(())
}
