use super::*;

/// Expiry drains sequence guards independently of when each reader drains its events.
#[test]
fn expiry_reclaims_runtime_cuts_without_discarding_queued_terminal_events()
-> Result<(), Box<dyn std::error::Error>> {
    let session = setup()?;
    let fast = session.subscribe().ok_or("fast subscriber")?;
    let slow = session.subscribe().ok_or("slow subscriber")?;
    let runtime = session.runtime();
    register(&runtime)?;
    let (sender, _receiver) = mpsc::channel();
    let mut jobs = Vec::new();
    for _ in 0..128 {
        let crate::SubmitResult::Created(id) = runtime.submit(crate::Submission {
            kind_id: "scan.metadata".into(),
            deduplication_key: None,
            body: Arc::new(Body(sender.clone())),
            priority: crate::JobPriority::Normal,
            scope_summary: "scope".into(),
            correlation_id: Identity(AtomicUsize::new(42)).next_id(),
            sample: false,
        })?
        else {
            return Err("new job expected".into());
        };
        runtime.cancel(&id)?;
        runtime.flush()?;
        while fast.try_next(Duration::from_millis(100))?.is_some() {}
        jobs.push(id);
    }
    for subscriber in [&fast, &slow] {
        assert_eq!(subscriber.subscription.retained_cut_count(), Some(128));
    }
    {
        let mut table = runtime.table.lock().map_err(|_| "table lock")?;
        crate::model::expire(&mut table, 599_999);
        assert_eq!(table.jobs.len(), 128);
        crate::model::expire(&mut table, 600_000);
        assert!(table.jobs.is_empty());
    }
    runtime.flush()?;
    for subscriber in [&fast, &slow] {
        assert_eq!(subscriber.subscription.retained_cut_count(), Some(0));
    }
    assert!(fast.try_next(Duration::from_secs(1))?.is_none());
    let mut terminal = Vec::new();
    while let Some(message) = slow.try_next(Duration::from_secs(1))? {
        match message {
            SubscriberMessage::Event(JobEvent::Finished { summary, .. }) => {
                terminal.push(summary.job_id);
            }
            SubscriberMessage::Event(_) => {}
            SubscriberMessage::ResyncRequired { .. } => {
                return Err("expiry must not detach the slow reader".into());
            }
        }
    }
    assert_eq!(terminal, jobs);
    Ok(())
}

/// Expiry cannot pass a terminal delivery that is paused in the downstream sink.
#[test]
fn expiry_waits_behind_an_in_flight_terminal_delivery() -> Result<(), Box<dyn std::error::Error>> {
    struct SlowTerminal {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl JobEvents for SlowTerminal {
        fn publish(&self, event: JobEvent) {
            if matches!(
                event,
                JobEvent::StateChanged {
                    state: JobState::Cancelled,
                    ..
                }
            ) {
                let _ = self.entered.send(());
                if let Ok(release) = self.release.lock() {
                    let _ = release.recv_timeout(Duration::from_secs(5));
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
        downstream: Arc::new(SlowTerminal {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        }),
        limits: FlowLimits::disabled(),
    })?;
    let subscriber = session.subscribe().ok_or("subscriber")?;
    let runtime = session.runtime();
    register(&runtime)?;
    let (sender, _receiver) = mpsc::channel();
    let crate::SubmitResult::Created(job) = runtime.submit(crate::Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(Body(sender)),
        priority: crate::JobPriority::Normal,
        scope_summary: "scope".into(),
        correlation_id: Identity(AtomicUsize::new(42)).next_id(),
        sample: false,
    })?
    else {
        return Err("new job expected".into());
    };
    runtime.cancel(&job)?;
    entered_rx.recv_timeout(Duration::from_secs(2))?;
    {
        let mut table = runtime.table.lock().map_err(|_| "table lock")?;
        crate::model::expire(&mut table, 600_000);
        assert!(!table.jobs.contains_key(&job));
    }
    assert_eq!(subscriber.subscription.retained_cut_count(), Some(1));
    release_tx.send(())?;
    runtime.flush()?;
    assert_eq!(subscriber.subscription.retained_cut_count(), Some(0));
    let mut terminal = None;
    while let Some(event) = subscriber.try_next(Duration::from_secs(1))? {
        if let SubscriberMessage::Event(JobEvent::Finished { summary, .. }) = event {
            assert!(terminal.replace(summary.job_id).is_none());
        }
    }
    assert_eq!(terminal, Some(job));
    Ok(())
}
