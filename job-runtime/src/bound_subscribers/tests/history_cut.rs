use super::*;
use du_core_contracts::job::{HistoryAppendOutcome, HistoryAppendReceipt, HistoryRevision};

struct BlockingHistory {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    succeeds: bool,
}
impl JobHistory for BlockingHistory {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Err("unexpected legacy append".into())
    }
    fn append_with_receipt(&self, _: &JobSummary) -> Result<HistoryAppendOutcome, String> {
        let _ = self.entered.send(());
        self.release
            .lock()
            .map_err(|_| "history lock")?
            .recv()
            .map_err(|_| "history release")?;
        if self.succeeds {
            Ok(HistoryAppendOutcome::Added(Some(HistoryAppendReceipt {
                revision: HistoryRevision {
                    delete_generation: 0,
                    max_record_seq: 1,
                },
                observed_at_ms: 0,
            })))
        } else {
            Err("db.busy".into())
        }
    }
}
#[derive(Default)]
struct Order {
    order: Mutex<Vec<&'static str>>,
    terminal: Mutex<Option<(bool, Option<String>)>>,
}
impl JobEvents for Order {
    fn publish(&self, event: JobEvent) {
        let label = match event {
            JobEvent::StateChanged {
                state: JobState::Done,
                ..
            } => Some("state"),
            JobEvent::Finished {
                history_recorded,
                warning,
                ..
            } => {
                if let Ok(mut terminal) = self.terminal.lock() {
                    *terminal = Some((history_recorded, warning));
                }
                Some("finished")
            }
            _ => None,
        };
        if let Some(label) = label
            && let Ok(mut order) = self.order.lock()
        {
            order.push(label);
        }
    }
    fn publish_history_changed(&self, _: HistoryChanged) {
        if let Ok(mut order) = self.order.lock() {
            order.push("history");
        }
    }
}

fn run_history_case(succeeds: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let sink = Arc::new(Order::default());
    let session = RuntimeSubscribers::new(RuntimeSubscribersConfig {
        identity: Arc::new(Identity(AtomicUsize::new(1))),
        runtime_clock: Arc::new(Clock),
        subscriber_clock: Arc::new(Clock),
        history: Arc::new(BlockingHistory {
            entered: entered_tx,
            release: Mutex::new(release_rx),
            succeeds,
        }),
        downstream: sink.clone(),
        limits: FlowLimits {
            metadata_io: 1,
            queued_items: 8,
            item_bytes: 32,
        },
    })?;
    let subscriber = session.subscribe().ok_or("subscriber")?;
    let runtime = session.runtime();
    runtime.register(JobDescriptor {
        kind_id: "scan.metadata".into(),
        version: 1,
        display_name_key: "scan".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::MetadataIo],
        history: HistoryMode::Summary,
    })?;
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
        _ => return Err("expected new job".into()),
    };
    fake_overflow(&session);
    let runner = {
        let runtime = runtime.clone();
        thread::spawn(move || runtime.run_next())
    };
    let flow = rx.recv_timeout(Duration::from_secs(2))?;
    flow.finish(JobCounts::default());
    runner.join().map_err(|_| "worker panic")??;
    entered_rx.recv_timeout(Duration::from_secs(2))?;
    let token = subscriber.begin_rejoin()?.ok_or("pending token")?;
    assert!(matches!(
        session.rejoin(&subscriber, token),
        Err(RuntimeError::DeliveryBusy)
    ));
    {
        let mut table = runtime.table.lock().map_err(|_| "table lock")?;
        crate::model::expire(&mut table, 700_000);
        assert!(table.jobs.contains_key(&job));
    }
    release_tx.send(())?;
    runtime.flush()?;
    let token = subscriber.begin_rejoin()?.ok_or("completed token")?;
    let snapshot = session.rejoin(&subscriber, token)?.ok_or("rejoin")?;
    let row = snapshot.get(&job).ok_or("job cut")?;
    assert_eq!(row.snapshot.state, JobState::Done);
    assert_eq!(row.history_recorded, Some(succeeds));
    assert_eq!(
        row.history_warning.as_deref(),
        if succeeds { None } else { Some("db.busy") }
    );
    assert!(row.summary.is_some());
    assert!(subscriber.try_next(Duration::from_secs(1))?.is_none());
    let terminal = sink.terminal.lock().map_err(|_| "terminal lock")?;
    assert_eq!(*terminal, Some((succeeds, row.history_warning.clone())));
    let observed = sink.order.lock().map_err(|_| "sink lock")?;
    if succeeds {
        assert_eq!(*observed, ["state", "finished", "history"]);
    } else {
        assert_eq!(*observed, ["state", "finished"]);
    }
    Ok(())
}

#[test]
fn history_success_is_committed_before_terminal_cut_and_notice()
-> Result<(), Box<dyn std::error::Error>> {
    run_history_case(true)
}
#[test]
fn history_failure_is_preserved_in_terminal_cut_without_notice()
-> Result<(), Box<dyn std::error::Error>> {
    run_history_case(false)
}

#[test]
fn blocked_downstream_cannot_make_pending_terminal_look_committed()
-> Result<(), Box<dyn std::error::Error>> {
    struct Slow {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl JobEvents for Slow {
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
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let session = RuntimeSubscribers::new(RuntimeSubscribersConfig {
        identity: Arc::new(Identity(AtomicUsize::new(1))),
        runtime_clock: Arc::new(Clock),
        subscriber_clock: Arc::new(Clock),
        history: Arc::new(History),
        downstream: Arc::new(Slow {
            entered: entered_tx,
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
    runtime.register(JobDescriptor {
        kind_id: "scan.metadata".into(),
        version: 1,
        display_name_key: "scan".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::MetadataIo],
        history: HistoryMode::Summary,
    })?;
    fake_overflow(&session);
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
        _ => return Err("expected new job".into()),
    };
    let runner = {
        let runtime = runtime.clone();
        thread::spawn(move || runtime.run_next())
    };
    let flow = rx.recv_timeout(Duration::from_secs(2))?;
    entered_rx.recv_timeout(Duration::from_secs(2))?;
    flow.finish(JobCounts::default());
    runner.join().map_err(|_| "worker panic")??;
    let token = subscriber.begin_rejoin()?.ok_or("pending token")?;
    assert!(matches!(
        session.rejoin(&subscriber, token),
        Err(RuntimeError::DeliveryBusy)
    ));
    release_tx.send(())?;
    runtime.flush()?;
    let token = subscriber.begin_rejoin()?.ok_or("ready token")?;
    let cut = session.rejoin(&subscriber, token)?.ok_or("cut")?;
    assert_eq!(cut.get(&job).ok_or("job")?.history_recorded, Some(true));
    Ok(())
}
