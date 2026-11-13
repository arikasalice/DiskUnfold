use super::super::super::contract::{
    DispatchLane, EventCallbacks, Events, ResponseDelivery, ResponseDeliveryCallback,
    ResponseDeliveryObserver, WebApp,
};
use super::super::folder_validation::{self, FolderVerifier};
use super::super::reply::Reply;
use super::*;
use crate::webview::contract::entry;
use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

fn app(
    dispatch: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    route: fn(&str) -> DispatchLane,
    response_delivery: Option<ResponseDeliveryObserver>,
) -> WebApp {
    WebApp {
        asset: |_| None,
        dispatch: Arc::new(dispatch),
        route,
        response_delivery,
        initialization_script: None,
        icon: &[],
    }
}

fn recv(owner: &ControllerOwner, timeout: Duration) -> io::Result<Reply> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(reply) = owner.take_reply() {
            return Ok(reply);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "reply timeout"));
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn recv_message(owner: &ControllerOwner, timeout: Duration) -> io::Result<String> {
    recv(owner, timeout).map(|reply| reply.message.clone())
}

fn ordered(_: &str) -> DispatchLane {
    DispatchLane::Ordered
}

#[test]
fn one_hwnd_pool_reuses_exactly_eight_workers_across_controller_owners()
-> Result<(), Box<dyn std::error::Error>> {
    let app = app(|request| Some(request.to_owned()), ordered, None);
    let pool = WorkerPool::start(&app)?;
    assert_eq!(pool.worker_count(), 8);

    for generation in 0..12 {
        let owner = pool.controller_owner()?;
        let requests = owner.requests(app.route);
        assert_eq!(
            requests.enqueue_ipc(&entry(), "1", &format!("echo-{generation}")),
            EnqueueResult::Accepted
        );
        assert_eq!(
            recv_message(&owner, Duration::from_secs(5))?,
            format!("1\necho-{generation}")
        );
        drop(owner);
        assert_eq!(pool.worker_count(), 8);
    }
    Ok(())
}

#[test]
fn revoked_queued_ipc_has_no_effect_and_blocked_reply_cannot_cross_reused_id()
-> Result<(), Box<dyn std::error::Error>> {
    let (started, blocked) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let expired_effects = Arc::new(AtomicUsize::new(0));
    let expired_effects_in_dispatch = Arc::clone(&expired_effects);
    let (outcomes, observed) = mpsc::channel();
    let observer: ResponseDeliveryObserver = Arc::new(move |request, _| {
        let outcomes = outcomes.clone();
        let request = request.to_owned();
        Some(Box::new(move |outcome| {
            let _ = outcomes.send((request, outcome));
        }) as ResponseDeliveryCallback)
    });
    let app = app(
        move |request| match request {
            "old.block" => {
                started.send(()).ok()?;
                gate.lock()
                    .ok()?
                    .recv_timeout(Duration::from_secs(5))
                    .ok()?;
                Some("old-response".into())
            }
            "old.queued" => {
                expired_effects_in_dispatch.fetch_add(1, Ordering::SeqCst);
                Some("expired-response".into())
            }
            other => Some(format!("response:{other}")),
        },
        ordered,
        Some(observer),
    );
    let pool = WorkerPool::start(&app)?;
    let old_owner = pool.controller_owner()?;
    let old_requests = old_owner.requests(app.route);
    assert_eq!(
        old_requests.enqueue_ipc(&entry(), "same-id", "old.block"),
        EnqueueResult::Accepted
    );
    blocked.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(
        old_requests.enqueue_ipc(&entry(), "same-id", "old.queued"),
        EnqueueResult::Accepted
    );

    let new_owner = pool.controller_owner()?;
    let new_requests = new_owner.requests(app.route);
    assert_eq!(
        old_requests.enqueue_ipc(&entry(), "same-id", "too-late"),
        EnqueueResult::Stale
    );
    assert_eq!(
        new_requests.enqueue_ipc(&entry(), "same-id", "new"),
        EnqueueResult::Accepted
    );
    release.send(())?;

    let posted = recv(&new_owner, Duration::from_secs(5))?;
    assert_eq!(posted.message, "same-id\nresponse:new");
    let (old_request, old_outcome) = observed.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(old_request, "old.block");
    assert_eq!(old_outcome, ResponseDelivery::NotPosted);
    posted.deliver_with(|wire| {
        assert_eq!(wire, "same-id\nresponse:new");
        Ok::<(), io::Error>(())
    })?;
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(5))?,
        ("new".to_owned(), ResponseDelivery::Posted)
    );
    assert!(observed.try_recv().is_err());
    assert_eq!(expired_effects.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn revoking_owner_discards_mailbox_reply_and_reports_not_posted_once()
-> Result<(), Box<dyn std::error::Error>> {
    let (outcomes, observed) = mpsc::channel();
    let observer: ResponseDeliveryObserver = Arc::new(move |request, _| {
        let outcomes = outcomes.clone();
        let request = request.to_owned();
        Some(Box::new(move |outcome| {
            let _ = outcomes.send((request, outcome));
        }) as ResponseDeliveryCallback)
    });
    let app = app(|request| Some(request.to_owned()), ordered, Some(observer));
    let pool = WorkerPool::start(&app)?;
    let old_owner = pool.controller_owner()?;
    assert_eq!(
        old_owner
            .requests(app.route)
            .enqueue_ipc(&entry(), "same", "old"),
        EnqueueResult::Accepted
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while old_owner.reply_count() == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(old_owner.reply_count(), 1);

    let new_owner = pool.controller_owner()?;
    assert_eq!(old_owner.reply_count(), 0);
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(5))?,
        ("old".to_owned(), ResponseDelivery::NotPosted)
    );
    assert!(observed.try_recv().is_err());
    assert_eq!(
        new_owner
            .requests(app.route)
            .enqueue_ipc(&entry(), "same", "new"),
        EnqueueResult::Accepted
    );
    assert_eq!(
        recv_message(&new_owner, Duration::from_secs(5))?,
        "same\nnew"
    );
    Ok(())
}

#[test]
fn observer_is_prepared_on_worker_and_notified_only_after_native_post()
-> Result<(), Box<dyn std::error::Error>> {
    let (prepared, preparations) = mpsc::channel();
    let (notified, notifications) = mpsc::channel();
    let observer: ResponseDeliveryObserver = Arc::new(move |request, response| {
        prepared
            .send((
                thread::current().id(),
                request.to_owned(),
                response.to_owned(),
            ))
            .ok()?;
        let notified = notified.clone();
        Some(Box::new(move |outcome| {
            let _ = notified.send((thread::current().id(), outcome));
        }))
    });
    let app = app(
        |request| Some(format!("receipt:{request}")),
        ordered,
        Some(observer),
    );
    let pool = WorkerPool::start(&app)?;
    let owner = pool.controller_owner()?;
    assert_eq!(
        owner
            .requests(app.route)
            .enqueue_ipc(&entry(), "22", "scan.start"),
        EnqueueResult::Accepted
    );
    let (worker_thread, request, response) = preparations.recv_timeout(Duration::from_secs(5))?;
    let posting_thread = thread::current().id();
    assert_ne!(worker_thread, posting_thread);
    assert_eq!(request, "scan.start");
    assert_eq!(response, "receipt:scan.start");
    let reply = recv(&owner, Duration::from_secs(5))?;
    assert!(notifications.try_recv().is_err());
    owner.queue_test_reply(reply);
    assert!(deliver_reply_batch(&owner, |wire| {
        assert_eq!(wire, "22\nreceipt:scan.start");
        Ok::<(), io::Error>(())
    }));
    assert_eq!(
        notifications.recv_timeout(Duration::from_secs(5))?,
        (posting_thread, ResponseDelivery::Posted)
    );
    Ok(())
}

#[test]
fn response_without_delivery_observer_still_reaches_native_post()
-> Result<(), Box<dyn std::error::Error>> {
    let app = app(|request| Some(request.to_owned()), ordered, None);
    let pool = WorkerPool::start(&app)?;
    let owner = pool.controller_owner()?;
    assert_eq!(
        owner
            .requests(app.route)
            .enqueue_ipc(&entry(), "21", "scan.receipt"),
        EnqueueResult::Accepted
    );
    let reply = recv(&owner, Duration::from_secs(5))?;
    let mut posted = false;
    owner.queue_test_reply(reply);
    assert!(deliver_reply_batch(&owner, |wire| {
        assert_eq!(wire, "21\nscan.receipt");
        posted = true;
        Ok::<(), io::Error>(())
    }));
    assert!(posted);
    Ok(())
}

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

#[test]
fn reentrant_owner_replacement_marks_posted_batch_as_not_posted_once()
-> Result<(), Box<dyn std::error::Error>> {
    let app = app(|request| Some(request.to_owned()), ordered, None);
    let pool = WorkerPool::start(&app)?;
    let owner = pool.controller_owner()?;
    let (outcomes, observed) = mpsc::channel();
    let callback = |id: &'static str| {
        let outcomes = outcomes.clone();
        Box::new(move |outcome| {
            let _ = outcomes.send((id, outcome));
        }) as ResponseDeliveryCallback
    };
    owner.queue_test_reply(Reply::ipc("same".into(), "first", Some(callback("first"))));
    owner.queue_test_reply(Reply::ipc(
        "same".into(),
        "second",
        Some(callback("second")),
    ));
    let event_reads = Arc::new(AtomicUsize::new(0));
    let event_source_reads = Arc::clone(&event_reads);
    let events = Events::new(
        move || {
            event_source_reads.fetch_add(1, Ordering::SeqCst);
            Some("event.generation.retired".into())
        },
        EventCallbacks::new(|_| {}, || {}, || {}, || {}),
    );
    let mut replacement = None;
    let mut posts = Vec::new();
    let active = deliver_reply_batch(&owner, |wire| {
        posts.push(wire.to_owned());
        replacement = Some(pool.controller_owner()?);
        Ok::<(), io::Error>(())
    });

    assert_eq!(posts, ["same\nfirst"]);
    assert!(!active);
    assert!(!flush_events_if_active(&owner, &events, |_| Ok::<
        (),
        io::Error,
    >(())));
    assert_eq!(event_reads.load(Ordering::SeqCst), 0);
    let mut notifications = observed.try_iter().collect::<Vec<_>>();
    notifications.sort_by_key(|(id, _)| *id);
    assert_eq!(
        notifications,
        [
            ("first", ResponseDelivery::NotPosted),
            ("second", ResponseDelivery::NotPosted),
        ]
    );
    drop(replacement);
    Ok(())
}

mod lanes;
mod post_failure;
mod reply_flush;
