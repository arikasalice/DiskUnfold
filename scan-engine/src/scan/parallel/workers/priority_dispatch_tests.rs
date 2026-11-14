use super::*;
use du_core_contracts::scan_observation::DirectoryObservations;

struct EmptyDirectory;

impl DirectoryObservations for EmptyDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}

impl AnchoredDirectory for EmptyDirectory {
    // Placed in a frame directly; root identities are never queried here.
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        None
    }
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Ok(Box::new(Self))
    }
}

fn frame(name: &[u8], marker: bool) -> Frame {
    Frame {
        root_index: 0,
        path: vec![name.to_vec()],
        directory: Box::new(EmptyDirectory),
        entries: VecDeque::new(),
        deferred: VecDeque::new(),
        root: false,
        first_batch: marker,
        root_published: false,
        batch_pending: false,
        case_observed: false,
        boundary: Boundary::Unknown,
    }
}

fn shared(shared_path: &[u8], priority_path: &[u8]) -> Shared {
    let ready = frame(shared_path, true);
    Shared {
        queue: Arc::new(Mutex::new(Queue {
            ready: VecDeque::from([ready]),
            ready_high_water: None,
            last_root: None,
            priority: Some(queue::PriorityRegion {
                root_index: 0,
                path: vec![priority_path.to_vec()],
            }),
            outstanding: None,
            remaining: 2,
            root_remaining: vec![2],
            root_groups: vec![0],
            active_groups: vec![0],
            device_limit: 1,
            started: true,
        })),
        wake: Arc::new(Condvar::new()),
        abort: AtomicBool::new(false),
        disconnects: disconnect::Disconnects::new(1, &[vec![1]]),
        track_priority_paths: true,
        visited: Default::default(),
    }
}

fn claim_path(
    shared: &Shared,
    local: &mut Vec<Frame>,
    prefer_shared: bool,
) -> Option<(Vec<Vec<u8>>, bool)> {
    let claim = {
        let mut queue = shared.queue.lock().ok()?;
        dispatch::claim_next_for_test(shared, &mut queue, local, prefer_shared)
    }?;
    Some((claim.frame.path.clone(), claim.frame.first_batch))
}

#[test]
fn higher_priority_candidate_wins_across_shared_and_local_queues() {
    let priority = b"B";

    let local_priority = shared(b"A", priority);
    assert_eq!(
        claim_path(&local_priority, &mut vec![frame(priority, false)], true),
        Some((vec![priority.to_vec()], false))
    );

    let shared_priority = shared(priority, priority);
    assert_eq!(
        claim_path(&shared_priority, &mut vec![frame(b"A", false)], false),
        Some((vec![priority.to_vec()], true))
    );
}

#[test]
fn equal_priority_candidates_keep_worker_alternation() -> Result<(), Box<dyn std::error::Error>> {
    let priority = b"B";

    let prefer_shared = shared(priority, priority);
    assert_eq!(
        claim_path(&prefer_shared, &mut vec![frame(priority, false)], true),
        Some((vec![priority.to_vec()], true))
    );

    let prefer_local = shared(priority, priority);
    assert_eq!(
        claim_path(&prefer_local, &mut vec![frame(priority, false)], false),
        Some((vec![priority.to_vec()], false))
    );
    let queue = prefer_local
        .queue
        .lock()
        .map_err(|_| "queue lock poisoned")?;
    assert_eq!(queue.ready.len(), 1);
    assert_eq!(queue.ready[0].path, [priority.to_vec()]);
    Ok(())
}

#[test]
fn default_dispatch_preserves_shared_alternation_and_local_lifo()
-> Result<(), Box<dyn std::error::Error>> {
    let shared_first = shared(b"A", b"B");
    shared_first
        .queue
        .lock()
        .map_err(|_| "queue lock poisoned")?
        .priority = None;
    assert_eq!(
        claim_path(&shared_first, &mut vec![frame(b"B", false)], true),
        Some((vec![b"A".to_vec()], true))
    );

    let local_first = shared(b"A", b"B");
    local_first
        .queue
        .lock()
        .map_err(|_| "queue lock poisoned")?
        .priority = None;
    assert_eq!(
        claim_path(&local_first, &mut vec![frame(b"B", false)], false),
        Some((vec![b"B".to_vec()], false))
    );

    let local_lifo = shared(b"A", b"B");
    {
        let mut queue = local_lifo.queue.lock().map_err(|_| "queue lock poisoned")?;
        queue.priority = None;
        queue.ready.clear();
    }
    assert_eq!(
        claim_path(
            &local_lifo,
            &mut vec![frame(b"A", false), frame(b"B", false)],
            true,
        ),
        Some((vec![b"B".to_vec()], false))
    );
    Ok(())
}
