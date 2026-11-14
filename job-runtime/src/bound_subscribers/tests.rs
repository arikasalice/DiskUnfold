use super::*;
use crate::model::JobEvent;
use du_core_contracts::job::{
    Checkpoint, HistoryAppend, HistoryMode, JobBody, JobContext, JobControl, JobCounts,
    JobDescriptor, JobFlow, JobOutcome, JobState, JobSummary, ProgressUnit, ResourceClass,
};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

struct Identity(AtomicUsize);
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        let n = self.0.fetch_add(1, Ordering::Relaxed);
        let mut bytes = [0u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = n as u8;
        Id::from_v7_bytes(bytes)
    }
}
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        0
    }
}
impl SubscriberClock for Clock {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}
struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}
#[derive(Default)]
struct Downstream(Mutex<Vec<JobEvent>>);
impl JobEvents for Downstream {
    fn publish(&self, event: JobEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}
fn setup() -> Result<Arc<RuntimeSubscribers>, RuntimeError> {
    Ok(Arc::new(RuntimeSubscribers::new(
        RuntimeSubscribersConfig {
            identity: Arc::new(Identity(AtomicUsize::new(1))),
            runtime_clock: Arc::new(Clock),
            subscriber_clock: Arc::new(Clock),
            history: Arc::new(History),
            downstream: Arc::new(Downstream::default()),
            limits: FlowLimits {
                metadata_io: 1,
                queued_items: 8,
                item_bytes: 32,
            },
        },
    )?))
}
fn register(runtime: &JobRuntime) -> Result<(), RuntimeError> {
    runtime.register(JobDescriptor {
        kind_id: "scan.metadata".into(),
        version: 1,
        display_name_key: "scan".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::MetadataIo],
        history: HistoryMode::None,
    })
}
struct Body(mpsc::Sender<Box<dyn JobFlow>>);
impl JobBody for Body {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        if let Ok(flow) = context.try_start_flow() {
            let _ = self.0.send(flow);
        }
        let _ = context.checkpoint() == Checkpoint::Cancel;
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
fn fake_overflow(session: &RuntimeSubscribers) {
    let mut b = [0; 16];
    b[6] = 0x70;
    b[8] = 0x80;
    b[15] = 99;
    let job = Id::from_v7_bytes(b);
    for sequence in 1..=1024 {
        assert!(session.bus.publish(
            JobEvent::StateChanged {
                job_id: job.clone(),
                state: JobState::Running,
                sequence
            },
            Duration::ZERO
        ));
    }
}

#[test]
fn runtime_bound_cut_completes_under_live_reporting_and_preserves_terminal_order()
-> Result<(), Box<dyn std::error::Error>> {
    let session = setup()?;
    let subscriber = session.subscribe().ok_or("subscriber")?;
    let runtime = session.runtime();
    register(&runtime)?;
    let (tx, rx) = mpsc::channel();
    let job = match runtime.submit(crate::Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(Body(tx)),
        priority: crate::JobPriority::Normal,
        scope_summary: "scope".into(),
        correlation_id: Identity(AtomicUsize::new(42)).next_id(),
        sample: false,
    })? {
        crate::SubmitResult::Created(id) => id,
        _ => return Err("expected a new job".into()),
    };
    let worker = {
        let runtime = runtime.clone();
        thread::spawn(move || runtime.run_next())
    };
    let flow = rx.recv_timeout(Duration::from_secs(2))?;
    runtime.flush()?;
    let _ = subscriber.try_next(Duration::from_millis(100))?;
    fake_overflow(&session);
    assert!(matches!(
        subscriber.try_next(Duration::from_millis(100))?,
        Some(SubscriberMessage::ResyncRequired { .. })
    ));
    let token = subscriber.begin_rejoin()?.ok_or("token")?;
    let running = Arc::new(AtomicBool::new(true));
    let reporter_running = running.clone();
    let (started, started_rx) = mpsc::channel();
    let reporter = thread::spawn(move || {
        let _ = flow.report_item(b"first");
        let _ = started.send(());
        while reporter_running.load(Ordering::Acquire) {
            let _ = flow.report_item(b"report");
            thread::sleep(Duration::from_millis(1));
        }
        flow.finish(JobCounts::default());
    });
    started_rx.recv_timeout(Duration::from_secs(2))?;
    let (done, result) = mpsc::channel();
    let s = session.clone();
    let rejoin = thread::spawn(move || {
        let result = s.rejoin(&subscriber, token);
        let _ = done.send((subscriber, result));
    });
    let (subscriber, snapshot) = result.recv_timeout(Duration::from_secs(2))?;
    let snapshot = snapshot?.ok_or("rejoin failed")?;
    let cut = snapshot
        .get(&job)
        .ok_or("missing job cut")?
        .snapshot
        .sequence;
    assert_eq!(
        runtime.cancel(&job)?,
        crate::CancelResult::Accepted(JobState::CancelRequested)
    );
    running.store(false, Ordering::Release);
    reporter.join().map_err(|_| "reporter panic")?;
    worker.join().map_err(|_| "worker panic")??;
    rejoin.join().map_err(|_| "rejoin panic")?;
    runtime.flush()?;
    let mut last = cut;
    let mut finished = false;
    while let Some(message) = subscriber.try_next(Duration::from_secs(1))? {
        let SubscriberMessage::Event(event) = message else {
            return Err("unexpected overflow".into());
        };
        let (sequence, end) = match event {
            JobEvent::StateChanged { sequence, .. } | JobEvent::ItemResult { sequence, .. } => {
                (sequence, false)
            }
            JobEvent::Finished { sequence, .. } => (sequence, true),
            _ => return Err("unsequenced event entered job bus".into()),
        };
        assert!(sequence > last);
        assert!(!finished);
        last = sequence;
        finished = end;
    }
    assert!(finished);
    assert_eq!(runtime.snapshot(&job)?.sequence, last);
    Ok(())
}

#[test]
fn stale_and_foreign_tickets_are_rejected_by_bound_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let a = setup()?;
    let b = setup()?;
    let sa = a.subscribe().ok_or("a")?;
    let sb = b.subscribe().ok_or("b")?;
    fake_overflow(&a);
    fake_overflow(&b);
    let foreign = sa.begin_rejoin()?.ok_or("foreign")?;
    assert!(b.rejoin(&sb, foreign)?.is_none());
    let old = sb.begin_rejoin()?.ok_or("old")?;
    let current = sb.begin_rejoin()?.ok_or("current")?;
    assert!(b.rejoin(&sb, current)?.is_some());
    fake_overflow(&b);
    assert!(b.rejoin(&sb, old)?.is_none());
    let fresh = sb.begin_rejoin()?.ok_or("fresh")?;
    assert!(b.rejoin(&sb, fresh)?.is_some());
    Ok(())
}

#[test]
fn result_and_history_notices_stay_on_the_existing_downstream() -> Result<(), String> {
    use du_core_contracts::{
        job::{HistoryChangeReason, HistoryRevision},
        result_events::{
            PublishedCompleteness, ResultVersionChange, ResultVersionChanged, ResultVersionNotice,
        },
    };
    #[derive(Default)]
    struct Notices {
        results: AtomicUsize,
        history: AtomicUsize,
    }
    impl JobEvents for Notices {
        fn publish(&self, event: JobEvent) {
            if matches!(event, JobEvent::ResultVersionChanged(_)) {
                self.results.fetch_add(1, Ordering::SeqCst);
            }
        }
        fn publish_history_changed(&self, _: HistoryChanged) {
            self.history.fetch_add(1, Ordering::SeqCst);
        }
    }
    let bus = Arc::new(SubscriberBus::default());
    let sub = bus.subscribe().ok_or("subscriber")?;
    let downstream = Arc::new(Notices::default());
    let fanout = Fanout {
        bus,
        clock: Arc::new(Clock),
        downstream: downstream.clone(),
        failed: Arc::new(AtomicBool::new(false)),
    };
    let id = Identity(AtomicUsize::new(1)).next_id();
    fanout.publish(JobEvent::ResultVersionChanged(Box::new(
        ResultVersionNotice {
            result_version: Some(id.clone()),
            body: ResultVersionChanged {
                lineage_id: id.clone(),
                source_ref: id,
                origin: "scan".into(),
                data_source: None,
                sequence: 1,
                change: ResultVersionChange::Published,
                previous_result_version: None,
                completeness: Some(PublishedCompleteness::Partial),
                observed_at: None,
            },
        },
    )));
    fanout.publish_history_changed(HistoryChanged {
        revision: HistoryRevision {
            delete_generation: 0,
            max_record_seq: 1,
        },
        reason: HistoryChangeReason::Appended,
        count: 1,
        observed_at_ms: 0,
        deleted_job_ids: Vec::new(),
    });
    assert_eq!(downstream.results.load(Ordering::SeqCst), 1);
    assert_eq!(downstream.history.load(Ordering::SeqCst), 1);
    assert!(sub.try_next(Duration::from_millis(100)).is_none());
    Ok(())
}

#[test]
fn delivery_failure_is_visible_to_subscriber() -> Result<(), Box<dyn std::error::Error>> {
    let session = setup()?;
    let sub = session.subscribe().ok_or("subscribe")?;
    session.failed.store(true, Ordering::Release);
    assert!(matches!(
        sub.try_next(Duration::ZERO),
        Err(RuntimeError::DeliveryUnavailable)
    ));
    assert!(matches!(
        sub.begin_rejoin(),
        Err(RuntimeError::DeliveryUnavailable)
    ));
    Ok(())
}

#[test]
fn queued_cancel_racing_cut_is_in_snapshot_or_delivered_after_it()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Barrier;
    let session = setup()?;
    let subscriber = session.subscribe().ok_or("subscriber")?;
    let runtime = session.runtime();
    register(&runtime)?;
    let (tx, _rx) = mpsc::channel();
    let job = match runtime.submit(crate::Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(Body(tx)),
        priority: crate::JobPriority::Normal,
        scope_summary: "scope".into(),
        correlation_id: Identity(AtomicUsize::new(42)).next_id(),
        sample: false,
    })? {
        crate::SubmitResult::Created(id) => id,
        _ => return Err("new job expected".into()),
    };
    fake_overflow(&session);
    let token = subscriber.begin_rejoin()?.ok_or("token")?;
    let start = Arc::new(Barrier::new(3));
    let cancel = {
        let runtime = runtime.clone();
        let job = job.clone();
        let start = start.clone();
        thread::spawn(move || {
            start.wait();
            runtime.cancel(&job)
        })
    };
    let rejoin = {
        let session = session.clone();
        let start = start.clone();
        thread::spawn(move || {
            start.wait();
            let cut = session.rejoin(&subscriber, token);
            (subscriber, cut)
        })
    };
    start.wait();
    let _ = cancel.join().map_err(|_| "cancel panic")??;
    let (subscriber, cut) = rejoin.join().map_err(|_| "rejoin panic")?;
    let cut = match cut {
        Err(RuntimeError::DeliveryBusy) => {
            runtime.flush()?;
            let token = subscriber.begin_rejoin()?.ok_or("retry token")?;
            session.rejoin(&subscriber, token)?
        }
        result => result?,
    }
    .ok_or("missing cut")?;
    let row = cut.get(&job).ok_or("missing job")?;
    runtime.flush()?;
    let mut finished = 0;
    while let Some(message) = subscriber.try_next(Duration::from_secs(1))? {
        let SubscriberMessage::Event(event) = message else {
            return Err("unexpected overflow".into());
        };
        match event {
            JobEvent::StateChanged {
                job_id, sequence, ..
            }
            | JobEvent::ItemResult {
                job_id, sequence, ..
            } => {
                assert_eq!(job_id, job);
                assert!(sequence > row.snapshot.sequence);
            }
            JobEvent::Finished {
                summary, sequence, ..
            } => {
                assert_eq!(summary.job_id, job);
                assert!(sequence > row.snapshot.sequence);
                finished += 1;
            }
            _ => return Err("unsequenced event entered job bus".into()),
        }
    }
    if row.snapshot.state.is_terminal() {
        assert_eq!(row.snapshot.state, JobState::Cancelled);
        assert!(row.summary.is_some());
        assert_eq!(finished, 0);
    } else {
        assert_eq!(finished, 1);
    }
    Ok(())
}

#[test]
fn blocked_legacy_downstream_delays_next_bus_delivery() -> Result<(), Box<dyn std::error::Error>> {
    struct Blocked {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl JobEvents for Blocked {
        fn publish(&self, event: JobEvent) {
            if matches!(
                event,
                JobEvent::StateChanged {
                    state: JobState::Running,
                    ..
                }
            ) {
                let _ = self.entered.send(());
                if let Ok(release) = self.release.lock() {
                    let _ = release.recv();
                }
            }
        }
    }
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let session = RuntimeSubscribers::new(RuntimeSubscribersConfig {
        identity: Arc::new(Identity(AtomicUsize::new(1))),
        runtime_clock: Arc::new(Clock),
        subscriber_clock: Arc::new(Clock),
        history: Arc::new(History),
        downstream: Arc::new(Blocked {
            entered,
            release: Mutex::new(release_rx),
        }),
        limits: FlowLimits {
            metadata_io: 1,
            queued_items: 8,
            item_bytes: 32,
        },
    })?;
    let subscriber = session.subscribe().ok_or("subscriber")?;
    let runtime = session.runtime();
    register(&runtime)?;
    let (tx, rx) = mpsc::channel();
    let job = match runtime.submit(crate::Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(Body(tx)),
        priority: crate::JobPriority::Normal,
        scope_summary: "scope".into(),
        correlation_id: Identity(AtomicUsize::new(42)).next_id(),
        sample: false,
    })? {
        crate::SubmitResult::Created(id) => id,
        _ => return Err("new job expected".into()),
    };
    let worker = {
        let runtime = runtime.clone();
        thread::spawn(move || runtime.run_next())
    };
    let flow = rx.recv_timeout(Duration::from_secs(2))?;
    entered_rx.recv_timeout(Duration::from_secs(2))?;
    assert!(matches!(
        subscriber.try_next(Duration::from_millis(100))?,
        Some(SubscriberMessage::Event(JobEvent::StateChanged {
            state: JobState::Running,
            ..
        }))
    ));
    assert_eq!(
        runtime.cancel(&job)?,
        crate::CancelResult::Accepted(JobState::CancelRequested)
    );
    assert!(subscriber.try_next(Duration::from_secs(1))?.is_none());
    release.send(())?;
    flow.finish(JobCounts::default());
    worker.join().map_err(|_| "worker panic")??;
    runtime.flush()?;
    assert!(matches!(
        subscriber.try_next(Duration::from_secs(1))?,
        Some(SubscriberMessage::Event(JobEvent::StateChanged {
            state: JobState::CancelRequested,
            ..
        }))
    ));
    Ok(())
}

mod actual_overflow;
mod history_cut;
mod retained_cuts;
mod terminal;
