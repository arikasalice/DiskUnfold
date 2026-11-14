use super::*;

#[test]
fn default_four_per_key_does_not_block_a_distinct_key() {
    let frame = |root_index| {
        Frame::root(
            root_index,
            Box::new(SmallRoot {
                fail: false,
                done: false,
                name: Vec::new(),
            }),
        )
    };
    let mut queue = Queue::for_test(vec![0, 0, 0, 0, 0, 1], 4);
    for root in 0..6 {
        assert!(queue.push_root(frame(root)));
    }
    let seats: Vec<_> = (0..6).map(|_| queue.lanes.register_seat()).collect();
    let mut roots = Vec::new();
    for &seat in &seats {
        if let Some(taken) = queue.claim_for(seat) {
            roots.push(taken.frame.root_index);
        }
    }
    // Lane 0 admits four workers; the fifth becomes lane 1's worker.
    assert_eq!(roots, [0, 5, 1, 2, 3]);
    assert_eq!(queue.lanes.lanes[0].members, 4);
    assert_eq!(queue.lanes.lanes[1].members, 1);
    assert_eq!(queue.lanes.seats[seats[5]].lane, None);
    // A lane 0 worker continues with its own lane's remaining root.
    assert_eq!(
        queue.claim_for(seats[0]).map(|taken| taken.frame.root_index),
        Some(4)
    );
    // Lane 1's worker finds no more work there and leaves; nothing is left
    // anywhere that a worker outside lane 0 may take.
    assert!(queue.claim_for(seats[1]).is_none());
    assert!(queue.claim_for(seats[5]).is_none());
    assert_eq!(queue.lanes.lanes[1].members, 0);
}

#[test]
fn saturated_device_does_not_block_other_device_and_releases_on_return()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::mpsc;
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    struct GateRoot {
        id: u8,
        events: mpsc::Sender<u8>,
        gate: Option<Arc<(Mutex<bool>, Condvar)>>,
        done: bool,
    }
    impl DirectoryObservations for GateRoot {
        fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
            if self.done {
                return Ok(Vec::new());
            }
            self.done = true;
            let _ = self.events.send(self.id);
            if let Some(gate) = &self.gate {
                let (lock, wake) = &**gate;
                let mut released = lock.lock().map_err(|_| PortError {
                    kind: PortErrorKind::Io,
                    reason_key: "os.error".into(),
                    os_code: None,
                })?;
                while !*released {
                    released = wake.wait(released).map_err(|_| PortError {
                        kind: PortErrorKind::Io,
                        reason_key: "os.error".into(),
                        os_code: None,
                    })?;
                }
            }
            Ok(Vec::new())
        }
    }
    impl AnchoredDirectory for GateRoot {
        fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            })
        }
    }
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots = vec![
        observed_root("C:\\A", 1),
        observed_root("C:\\B", 1),
        observed_root("D:\\C", 2),
    ];
    let build = sink.open_forest(ids.next_id(), roots);
    let (send, recv) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let dirs = (0..3)
        .map(|id| {
            Box::new(GateRoot {
                id,
                events: send.clone(),
                gate: (id == 0).then(|| gate.clone()),
                done: false,
            }) as Box<dyn AnchoredDirectory>
        })
        .collect();
    let completed = Arc::new(AtomicUsize::new(0));
    let abandoned = Arc::new(AtomicUsize::new(0));
    let context = Context {
        completed: completed.clone(),
        abandoned: abandoned.clone(),
    };
    let outcome = std::thread::scope(|scope| {
        let task = scope.spawn(|| {
            collect_open_forest_with_groups(
                build,
                dirs,
                &ids,
                &context,
                3,
                &|_| true,
                DevicePlan {
                    groups: vec![0, 0, 1],
                    limit: 1,
                },
            )
        });
        let first = recv.recv_timeout(Duration::from_secs(2));
        let second = recv.recv_timeout(Duration::from_secs(2));
        let none_before_release = recv.try_recv().is_err();
        let (lock, wake) = &*gate;
        if let Ok(mut released) = lock.lock() {
            *released = true;
            wake.notify_all();
        }
        let third = recv.recv_timeout(Duration::from_secs(2));
        let outcome = task.join();
        (first, second, none_before_release, third, outcome)
    });
    let mut initial = [outcome.0?, outcome.1?];
    initial.sort();
    assert_eq!(initial, [0, 2]);
    assert!(outcome.2);
    assert_eq!(outcome.3?, 1);
    assert!(
        outcome
            .4
            .map_err(|_| "worker panic")?
            .result_version_id
            .is_some()
    );
    assert_eq!(completed.load(Ordering::Acquire), 4);
    assert_eq!(abandoned.load(Ordering::Acquire), 0);
    Ok(())
}
