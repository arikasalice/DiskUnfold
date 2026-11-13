//! Folder-drop validation tests: worker claims, revocation and the bounded waiting queue.

use super::*;

#[test]
fn revoked_folder_queue_is_purged_and_inflight_completion_is_not_delivered()
-> Result<(), Box<dyn std::error::Error>> {
    const BLOCKED_PATH: u16 = 7;
    let (started, started_rx) = mpsc::channel();
    let (finished, finished_rx) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Arc::new(Mutex::new(gate));
    let effects = Arc::new(Mutex::new(Vec::new()));
    let verifier_effects = Arc::clone(&effects);
    let verifier: Arc<FolderVerifier> = Arc::new(move |id, path| {
        let blocked = path.first() == Some(&BLOCKED_PATH);
        if blocked {
            let _ = started.send(id);
            if let Ok(gate) = gate.lock() {
                let _ = gate.recv_timeout(Duration::from_secs(5));
            }
        }
        if let Ok(mut effects) = verifier_effects.lock() {
            effects.push(id);
        }
        if blocked {
            let _ = finished.send(id);
        }
        format!(
            "folder-drop:{id}:{}",
            path.first().copied().unwrap_or_default()
        )
    });
    let app = app(|request| Some(request.to_owned()), ordered, None);
    let pool = WorkerPool::start_with_folder_verifier(&app, verifier)?;
    let old_owner = pool.controller_owner()?;
    let old_requests = old_owner.requests(app.route);

    // Occupy every folder worker so the next drops stay in the waiting queue.
    let in_flight: Vec<u64> = (1..=folder_validation::WORKER_COUNT as u64).collect();
    for id in &in_flight {
        assert_eq!(
            old_requests.enqueue_folder(*id, vec![BLOCKED_PATH]),
            EnqueueResult::Accepted
        );
    }
    let mut claimed: Vec<u64> = (0..in_flight.len())
        .map(|_| started_rx.recv_timeout(Duration::from_secs(5)))
        .collect::<Result<_, _>>()?;
    claimed.sort_unstable();
    assert_eq!(claimed, in_flight);
    let waiting: Vec<u64> = (100..100 + folder_validation::WAITING_LIMIT as u64).collect();
    for id in &waiting {
        assert_eq!(
            old_requests.enqueue_folder(*id, vec![*id as u16]),
            EnqueueResult::Accepted
        );
    }

    // A new controller revokes the old generation before any worker is released.
    let new_owner = pool.controller_owner()?;
    for _ in &in_flight {
        release.send(())?;
    }
    assert_eq!(
        old_requests.enqueue_folder(99, vec![99]),
        EnqueueResult::Stale
    );
    assert_eq!(
        new_owner.requests(app.route).enqueue_folder(1, vec![42]),
        EnqueueResult::Accepted
    );
    assert_eq!(
        recv_message(&new_owner, Duration::from_secs(5))?,
        "folder-drop:1:42"
    );
    // Wait for the revoked in-flight work to finish, then prove none of it was delivered.
    for _ in &in_flight {
        finished_rx.recv_timeout(Duration::from_secs(5))?;
    }
    assert!(
        recv_message(&new_owner, Duration::from_millis(200)).is_err(),
        "a revoked in-flight completion must not be delivered"
    );
    let effects = effects.lock().map_err(|_| "folder effect lock poisoned")?;
    assert!(
        waiting.iter().all(|id| !effects.contains(id)),
        "revoked waiting drops must never be verified: {effects:?}"
    );
    let mut expected = in_flight.clone();
    expected.push(1);
    let mut observed = effects.clone();
    observed.sort_unstable();
    expected.sort_unstable();
    assert_eq!(observed, expected);
    Ok(())
}

#[test]
fn slow_folder_validation_does_not_block_a_later_drop() -> Result<(), Box<dyn std::error::Error>> {
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let verifier: Arc<FolderVerifier> = Arc::new(move |id, _| {
        if id == 1 {
            let _ = entered.send(());
            if let Ok(gate) = gate.lock() {
                let _ = gate.recv_timeout(Duration::from_secs(5));
            }
        }
        format!("folder-drop:{id}")
    });
    let app = app(|request| Some(request.to_owned()), ordered, None);
    let pool = WorkerPool::start_with_folder_verifier(&app, verifier)?;
    let owner = pool.controller_owner()?;
    let requests = owner.requests(app.route);
    assert_eq!(requests.enqueue_folder(1, vec![1]), EnqueueResult::Accepted);
    started.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(requests.enqueue_folder(2, vec![2]), EnqueueResult::Accepted);
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "folder-drop:2"
    );
    release.send(())?;
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "folder-drop:1"
    );
    Ok(())
}

#[test]
fn folder_waiting_queue_stays_bounded_when_all_four_workers_block()
-> Result<(), Box<dyn std::error::Error>> {
    let (entered, started) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let worker_gate = Arc::clone(&gate);
    let verifier: Arc<FolderVerifier> = Arc::new(move |id, _| {
        let _ = entered.send(id);
        if let Ok(released) = worker_gate.0.lock() {
            let _ =
                worker_gate
                    .1
                    .wait_timeout_while(released, Duration::from_secs(5), |released| !*released);
        }
        format!("folder-drop:{id}")
    });
    let app = app(|request| Some(request.to_owned()), ordered, None);
    let pool = WorkerPool::start_with_folder_verifier(&app, verifier)?;
    let owner = pool.controller_owner()?;
    let requests = owner.requests(app.route);
    for id in 0..folder_validation::WORKER_COUNT as u64 {
        assert_eq!(
            requests.enqueue_folder(id, vec![id as u16]),
            EnqueueResult::Accepted
        );
    }
    for _ in 0..folder_validation::WORKER_COUNT {
        started.recv_timeout(Duration::from_secs(5))?;
    }
    for id in folder_validation::WORKER_COUNT as u64
        ..(folder_validation::WORKER_COUNT + folder_validation::WAITING_LIMIT) as u64
    {
        assert_eq!(
            requests.enqueue_folder(id, vec![id as u16]),
            EnqueueResult::Accepted
        );
    }
    assert_eq!(requests.enqueue_folder(99, vec![99]), EnqueueResult::Full);
    if let Ok(mut released) = gate.0.lock() {
        *released = true;
        gate.1.notify_all();
    }
    for _ in 0..folder_validation::WAITING_LIMIT {
        started.recv_timeout(Duration::from_secs(5))?;
    }
    Ok(())
}
