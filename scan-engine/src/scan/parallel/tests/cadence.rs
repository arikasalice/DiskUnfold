use super::*;

struct Clock {
    values: Mutex<VecDeque<Duration>>,
}
impl MonotonicClock for Clock {
    fn now(&self) -> Option<Duration> {
        Some(
            self.values
                .lock()
                .ok()
                .and_then(|mut values| values.pop_front())
                .unwrap_or_default(),
        )
    }
}

fn file_entry(name: &[u8], inode: u64) -> ObservedEntry {
    ObservedEntry {
        name: name.to_vec(),
        enumerated_inode: inode,
        enumerated_kind: ObservedKind::File,
        metadata: Ok(MetadataObservation {
            kind: ObservedKind::File,
            device: 1,
            inode,
            size: Ok(5),
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
        }),
    }
}

#[test]
fn timed_publications_pin_independent_versions_and_failed_build_rolls_back()
-> Result<(), Box<dyn std::error::Error>> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let mut prior = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
    prior
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let baseline = prior
        .seal(ids.next_id(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut build = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let clock = Clock {
        values: Mutex::new(VecDeque::from([
            Duration::ZERO,
            Duration::from_millis(100),
            Duration::from_millis(1100),
            Duration::from_millis(1800),
        ])),
    };
    let mut progress = Progress {
        failed_roots: vec![false],
        root_published: vec![false],
        published: false,
        cadence: PublicationCadence::default(),
    };
    progress
        .record(
            Event::RootBatch(0, None),
            &mut build,
            &mut JobCounts::default(),
            &context,
            &ids,
            &clock,
        )
        .map_err(|e| format!("{e:?}"))?;
    let first = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing first partial")?;
    assert_eq!(progress.cadence.remaining(Duration::from_secs(20)), None);
    let path = vec![native("same")];
    progress
        .record(
            Event::Item(0, path.clone(), Box::new(file_entry(&path[0], 7))),
            &mut build,
            &mut JobCounts::default(),
            &context,
            &ids,
            &clock,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        progress.cadence.remaining(Duration::from_millis(1099)),
        Some(Duration::from_millis(1))
    );
    assert_eq!(
        progress.cadence.remaining(Duration::from_millis(1100)),
        Some(Duration::ZERO)
    );
    progress
        .publish(&mut build, &context, &ids, &clock)
        .map_err(|e| format!("{e:?}"))?;
    let second = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing second partial")?;
    assert_ne!(first.version_id(), second.version_id());
    assert_eq!(first.items().len(), 1);
    assert_eq!(second.items().len(), 2);
    assert_eq!(progress.cadence.remaining(Duration::from_secs(10)), None);
    progress.cadence.changed();
    assert_eq!(
        progress.cadence.remaining(Duration::from_millis(3199)),
        Some(Duration::from_millis(1))
    );
    assert_eq!(
        progress.cadence.remaining(Duration::from_millis(3200)),
        Some(Duration::ZERO)
    );
    assert!(
        progress
            .record(
                Event::Item(0, path.clone(), Box::new(file_entry(&path[0], 7))),
                &mut build,
                &mut JobCounts::default(),
                &context,
                &ids,
                &clock
            )
            .is_err()
    );
    drop(build);
    let restored = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing baseline")?;
    assert_eq!(restored.version_id(), baseline.version_id());
    assert_eq!(second.items().len(), 2);
    Ok(())
}

struct SignalingClock {
    millis: std::sync::atomic::AtomicU64,
    calls: AtomicUsize,
    signal: mpsc::Sender<usize>,
}
impl MonotonicClock for SignalingClock {
    fn now(&self) -> Option<Duration> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let now = Duration::from_millis(self.millis.load(Ordering::SeqCst));
        if call == 4 || call == 7 {
            let _ = self.signal.send(call);
        }
        Some(now)
    }
}

#[test]
fn publishes_after_deadline_without_another_observation() -> Result<(), Box<dyn std::error::Error>>
{
    let ids = Arc::new(Ids(AtomicUsize::new(1)));
    let sink = IngestSink::default();
    let mut build = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let context = Arc::new(Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    });
    let (signal_tx, signal_rx) = mpsc::channel();
    let clock = Arc::new(SignalingClock {
        millis: std::sync::atomic::AtomicU64::new(0),
        calls: AtomicUsize::new(0),
        signal: signal_tx,
    });
    let (sender, receiver) = mpsc::channel();
    let worker_clock = Arc::clone(&clock);
    let worker_ids = Arc::clone(&ids);
    let worker = thread::spawn(move || {
        let mut progress = Progress {
            failed_roots: vec![false],
            root_published: vec![false],
            published: false,
            cadence: PublicationCadence::default(),
        };
        let abort = AtomicBool::new(false);
        let services = Services {
            context: context.as_ref(),
            ids: worker_ids.as_ref(),
            clock: worker_clock.as_ref(),
            abort: &abort,
        };
        consume(
            receiver,
            &mut progress,
            &mut build,
            &mut JobCounts::default(),
            services,
        )
        .map_err(|e| format!("{e:?}"))?;
        build
            .seal(worker_ids.next_id(), ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        Ok::<(), String>(())
    });
    sender
        .send(Event::RootBatch(0, None))
        .map_err(|_| "send root")?;
    let path = vec![native("later")];
    sender
        .send(Event::Item(
            0,
            path.clone(),
            Box::new(file_entry(&path[0], 7)),
        ))
        .map_err(|_| "send item")?;
    assert_eq!(signal_rx.recv_timeout(Duration::from_secs(2))?, 4);
    let first = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("first partial")?;
    clock.millis.store(1000, Ordering::SeqCst);
    assert_eq!(signal_rx.recv_timeout(Duration::from_secs(3))?, 7);
    let second = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("second partial")?;
    assert_ne!(first.version_id(), second.version_id());
    assert_eq!(second.items().len(), 2);
    drop(sender);
    worker.join().map_err(|_| "publisher thread panicked")??;
    let final_result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("final result")?;
    assert_eq!(first.items().len(), 1);
    assert_eq!(final_result.items().len(), 2);
    assert!(clock.calls.load(Ordering::SeqCst) >= 8);
    Ok(())
}

#[test]
fn abort_and_cancel_do_not_publish_queued_batches() -> Result<(), Box<dyn std::error::Error>> {
    struct Cancelled;
    impl JobContext for Cancelled {
        fn checkpoint(&self) -> Checkpoint {
            Checkpoint::Cancel
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Err(JobFlowError::Cancelled)
        }
    }
    let ids = Ids(AtomicUsize::new(1));
    let clock = Clock {
        values: Mutex::new(VecDeque::new()),
    };
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    for (aborted, ctx) in [
        (true, &context as &dyn JobContext),
        (false, &Cancelled as &dyn JobContext),
    ] {
        let sink = IngestSink::default();
        let mut build = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
        build
            .add_directory(Vec::new())
            .map_err(|e| format!("{e:?}"))?;
        let mut progress = Progress {
            failed_roots: vec![false],
            root_published: vec![false],
            published: false,
            cadence: PublicationCadence::default(),
        };
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Event::RootBatch(0, None))
            .map_err(|_| "send root")?;
        drop(sender);
        let abort = AtomicBool::new(aborted);
        let services = Services {
            context: ctx,
            ids: &ids,
            clock: &clock,
            abort: &abort,
        };
        consume(
            receiver,
            &mut progress,
            &mut build,
            &mut JobCounts::default(),
            services,
        )
        .map_err(|e| format!("{e:?}"))?;
        assert!(sink.pin().map_err(|e| format!("{e:?}"))?.is_none());
    }
    Ok(())
}
