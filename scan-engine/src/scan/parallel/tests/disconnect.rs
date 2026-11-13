use super::*;

fn disconnected() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "scan.disconnected".into(),
        os_code: None,
    }
}

fn entry(name: &str, kind: ObservedKind) -> ObservedEntry {
    ObservedEntry {
        name: native(name),
        enumerated_inode: 7,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device: 1,
            inode: 7,
            size: Ok(5),
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
            native_modified_at: None,
        }),
    }
}

struct ScriptedRoot {
    batches: VecDeque<Result<Vec<ObservedEntry>, PortError>>,
    calls: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
}

impl DirectoryObservations for ScriptedRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        self.batches.pop_front().unwrap_or_else(|| Ok(Vec::new()))
    }
}

impl AnchoredDirectory for ScriptedRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        self.opens.fetch_add(1, Ordering::AcqRel);
        Err(disconnected())
    }
}

#[test]
fn metadata_disconnect_drains_returned_items_and_closes_only_matching_volume()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots = vec![
        observed_root("C:\\A", 1),
        observed_root("C:\\B", 1),
        observed_root("D:\\C", 2),
    ];
    let mut missing = entry("lost", ObservedKind::File);
    missing.metadata = Err(disconnected());
    let batches = [
        vec![
            entry("folder", ObservedKind::Directory),
            entry("held", ObservedKind::File),
            missing,
        ],
        vec![entry("unread", ObservedKind::File)],
        vec![entry("healthy", ObservedKind::File)],
    ];
    let calls: Vec<_> = (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();
    let opens = Arc::new(AtomicUsize::new(0));
    let dirs = batches
        .into_iter()
        .zip(&calls)
        .map(|(batch, calls)| {
            Box::new(ScriptedRoot {
                batches: VecDeque::from([Ok(batch)]),
                calls: Arc::clone(calls),
                opens: Arc::clone(&opens),
            }) as Box<dyn AnchoredDirectory>
        })
        .collect();
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let build = sink.open_forest(ids.next_id(), roots);
    let outcome = collect_open_forest_with_groups(
        build,
        dirs,
        &ids,
        &context,
        1,
        &|_| true,
        DevicePlan {
            groups: vec![0, 0, 0],
            limit: 1,
        },
    );
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing result")?;
    assert_eq!(
        outcome.result_version_id.as_ref(),
        Some(result.version_id())
    );
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    assert_eq!(
        calls
            .iter()
            .map(|c| c.load(Ordering::Acquire))
            .collect::<Vec<_>>(),
        [1, 0, 2]
    );
    assert_eq!(opens.load(Ordering::Acquire), 0);
    for name in ["folder", "held", "lost", "healthy"] {
        assert!(
            result.items().iter().any(|item| item.name == native(name)),
            "missing {name}"
        );
    }
    assert!(
        !result
            .items()
            .iter()
            .any(|item| item.name == native("unread"))
    );
    assert!(
        result
            .scope_gaps()
            .iter()
            .any(|gap| gap.root_index == 1 && gap.reason_key == "scan.disconnected")
    );
    assert!(!result.scope_gaps().iter().any(|gap| gap.root_index == 2));
    Ok(())
}

#[test]
fn missing_volume_ids_do_not_share_a_shutdown_key() {
    let keys = super::super::disconnect::Disconnects::new(4, &[vec![], vec![7], vec![7]]);
    keys.close(0);
    keys.close(2);
    assert!(keys.closed(0));
    assert!(keys.closed(1));
    assert!(keys.closed(2));
    assert!(!keys.closed(3));
}

struct GatedRoot {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    reads: Arc<AtomicUsize>,
}

impl DirectoryObservations for GatedRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if self.reads.fetch_add(1, Ordering::AcqRel) != 0 {
            return Ok(Vec::new());
        }
        let _ = self.entered.send(());
        self.release
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| PortError {
                kind: PortErrorKind::Io,
                reason_key: "test.gate_timeout".into(),
                os_code: None,
            })?;
        Ok(vec![entry("in-flight", ObservedKind::File)])
    }
}

impl AnchoredDirectory for GatedRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(disconnected())
    }
}

#[test]
fn a_batch_already_in_flight_is_drained_after_another_root_disconnects()
-> Result<(), Box<dyn std::error::Error>> {
    let (entered, wait_entered) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    let (events, received) = mpsc::sync_channel(16);
    let reads = Arc::new(AtomicUsize::new(0));
    let roots: Vec<Box<dyn AnchoredDirectory>> = vec![
        Box::new(GatedRoot {
            entered,
            release: wait_release,
            reads: Arc::clone(&reads),
        }),
        Box::new(ScriptedRoot {
            batches: VecDeque::from([Err(disconnected())]),
            calls: Arc::new(AtomicUsize::new(0)),
            opens: Arc::new(AtomicUsize::new(0)),
        }),
    ];
    let shared = Shared {
        queue: Mutex::new(Queue {
            ready_high_water: Some(2),
            ready: roots
                .into_iter()
                .enumerate()
                .map(|(root, directory)| Frame {
                    root_index: root as u32,
                    path: Vec::new(),
                    directory,
                    entries: VecDeque::new(),
                    deferred: VecDeque::new(),
                    root: true,
                    first_batch: false,
                    root_published: false,
                    batch_pending: false,
                    case_observed: false,
                })
                .collect(),
            last_root: None,
            remaining: 2,
            root_remaining: vec![1; 2],
            root_groups: vec![0; 2],
            active_groups: vec![0; 2],
            device_limit: 2,
            started: true,
        }),
        wake: Condvar::new(),
        abort: AtomicBool::new(false),
        disconnects: super::super::disconnect::Disconnects::new(2, &[vec![1], vec![1]]),
    };
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let policy = Policy::Legacy(&|_| true);
    thread::scope(|scope| -> Result<(), Box<dyn std::error::Error>> {
        let first = context.try_start_flow().map_err(|_| "first flow")?;
        scope.spawn(|| worker(&shared, events.clone(), &context, policy, first));
        wait_entered.recv_timeout(Duration::from_secs(2))?;
        let second = context.try_start_flow().map_err(|_| "second flow")?;
        scope.spawn(|| worker(&shared, events.clone(), &context, policy, second));
        let failure = received.recv_timeout(Duration::from_secs(2));
        release.send(())?;
        assert!(matches!(
            failure?,
            Event::Failure {
                root_index: 1,
                reason: "scan.disconnected",
                ..
            }
        ));
        Ok(())
    })?;
    assert_eq!(reads.load(Ordering::Acquire), 1);
    let rest: Vec<_> = received.try_iter().collect();
    assert!(rest.iter().any(
        |event| matches!(event, Event::Item(0, path, _) if path == &vec![native("in-flight")])
    ));
    assert!(rest.iter().any(|event| matches!(
        event,
        Event::Failure {
            root_index: 0,
            reason: "scan.disconnected",
            ..
        }
    )));
    assert!(!shared.abort.load(Ordering::Acquire));
    Ok(())
}
