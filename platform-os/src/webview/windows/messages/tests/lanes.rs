use super::*;
#[test]
fn blocked_volume_lane_does_not_queue_scope_review_behind_it()
-> Result<(), Box<dyn std::error::Error>> {
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let app = app(
        move |command| {
            if command == "list" {
                entered.send(()).ok()?;
                gate.lock()
                    .ok()?
                    .recv_timeout(Duration::from_secs(5))
                    .ok()?;
            }
            Some(command.to_owned())
        },
        |command| {
            if command == "list" {
                DispatchLane::Volume
            } else {
                DispatchLane::Ordered
            }
        },
        None,
    );
    let pool = WorkerPool::start(&app)?;
    let owner = pool.controller_owner()?;
    let requests = owner.requests(app.route);
    assert_eq!(
        requests.enqueue_ipc(&entry(), "1", "list"),
        EnqueueResult::Accepted
    );
    started.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(
        requests.enqueue_ipc(&entry(), "2", "review"),
        EnqueueResult::Accepted
    );
    assert_eq!(recv_message(&owner, Duration::from_secs(5))?, "2\nreview");
    release.send(())?;
    assert_eq!(recv_message(&owner, Duration::from_secs(5))?, "1\nlist");
    Ok(())
}

#[test]
fn ordered_lane_keeps_close_behind_open() -> Result<(), Box<dyn std::error::Error>> {
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let app = app(
        move |command| {
            if command == "browse.open" {
                entered.send(()).ok()?;
                gate.lock()
                    .ok()?
                    .recv_timeout(Duration::from_secs(5))
                    .ok()?;
            }
            Some(command.to_owned())
        },
        |command| {
            if command == "target.list" {
                DispatchLane::Volume
            } else {
                DispatchLane::Ordered
            }
        },
        None,
    );
    let pool = WorkerPool::start(&app)?;
    let owner = pool.controller_owner()?;
    let requests = owner.requests(app.route);
    assert_eq!(
        requests.enqueue_ipc(&entry(), "1", "browse.open"),
        EnqueueResult::Accepted
    );
    started.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(
        requests.enqueue_ipc(&entry(), "2", "query.region.close"),
        EnqueueResult::Accepted
    );
    assert!(matches!(
        recv(&owner, Duration::from_millis(100)),
        Err(error) if error.kind() == io::ErrorKind::TimedOut
    ));
    release.send(())?;
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "1\nbrowse.open"
    );
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "2\nquery.region.close"
    );
    Ok(())
}

#[test]
fn query_controls_overtake_blocked_lanes_without_reordering_normal_requests()
-> Result<(), Box<dyn std::error::Error>> {
    let (entered, started) = mpsc::channel();
    let (release_open, open_gate) = mpsc::channel();
    let (release_volume, volume_gate) = mpsc::channel();
    let open_gate = Mutex::new(open_gate);
    let volume_gate = Mutex::new(volume_gate);
    let closed = Arc::new(AtomicUsize::new(0));
    let closed_state = Arc::clone(&closed);
    let app = app(
        move |command| match command {
            "browse.open" => {
                entered.send("open").ok()?;
                open_gate
                    .lock()
                    .ok()?
                    .recv_timeout(Duration::from_secs(5))
                    .ok()?;
                Some("published".into())
            }
            "target.list" => {
                entered.send("volume").ok()?;
                volume_gate
                    .lock()
                    .ok()?
                    .recv_timeout(Duration::from_secs(5))
                    .ok()?;
                Some("volume".into())
            }
            "query.cancel" => Some("ref.unavailable".into()),
            "query.region.close" => {
                closed_state.store(1, Ordering::Release);
                Some("closed".into())
            }
            other => Some(other.to_owned()),
        },
        |command| match command {
            "target.list" => DispatchLane::Volume,
            "query.cancel" | "query.region.close" => DispatchLane::QueryControl,
            _ => DispatchLane::Ordered,
        },
        None,
    );
    let pool = WorkerPool::start(&app)?;
    let owner = pool.controller_owner()?;
    let requests = owner.requests(app.route);
    for (id, command) in [("1", "browse.open"), ("2", "target.list")] {
        assert_eq!(
            requests.enqueue_ipc(&entry(), id, command),
            EnqueueResult::Accepted
        );
    }
    let mut both = [
        started.recv_timeout(Duration::from_secs(5))?,
        started.recv_timeout(Duration::from_secs(5))?,
    ];
    both.sort();
    assert_eq!(both, ["open", "volume"]);

    for (id, command) in [
        ("3", "normal.first"),
        ("4", "normal.second"),
        ("5", "query.cancel"),
    ] {
        assert_eq!(
            requests.enqueue_ipc(&entry(), id, command),
            EnqueueResult::Accepted
        );
    }
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "5\nref.unavailable"
    );
    assert_eq!(closed.load(Ordering::Acquire), 0);
    assert_eq!(
        requests.enqueue_ipc(&entry(), "6", "query.region.close"),
        EnqueueResult::Accepted
    );
    assert_eq!(recv_message(&owner, Duration::from_secs(5))?, "6\nclosed");
    release_open.send(())?;
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "1\npublished"
    );
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "3\nnormal.first"
    );
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "4\nnormal.second"
    );
    release_volume.send(())?;
    assert_eq!(recv_message(&owner, Duration::from_secs(5))?, "2\nvolume");
    Ok(())
}

#[test]
fn job_controls_keep_order_while_review_and_query_control_are_blocked()
-> Result<(), Box<dyn std::error::Error>> {
    let (entered, started) = mpsc::channel();
    let (release_review, review_gate) = mpsc::channel();
    let (release_query, query_gate) = mpsc::channel();
    let review_gate = Mutex::new(review_gate);
    let query_gate = Mutex::new(query_gate);
    let app = app(
        move |command| {
            let gate = match command {
                "scope.review" => Some(&review_gate),
                "query.region.close" => Some(&query_gate),
                _ => None,
            };
            if let Some(gate) = gate {
                entered.send(command.to_owned()).ok()?;
                gate.lock()
                    .ok()?
                    .recv_timeout(Duration::from_secs(5))
                    .ok()?;
            }
            Some(command.to_owned())
        },
        |command| match command {
            "job.pause" | "job.resume" | "job.cancel" => DispatchLane::JobControl,
            "query.region.close" => DispatchLane::QueryControl,
            _ => DispatchLane::Ordered,
        },
        None,
    );
    let pool = WorkerPool::start(&app)?;
    let owner = pool.controller_owner()?;
    let requests = owner.requests(app.route);
    for (id, command) in [("1", "scope.review"), ("2", "query.region.close")] {
        assert_eq!(
            requests.enqueue_ipc(&entry(), id, command),
            EnqueueResult::Accepted
        );
    }
    let mut both = [
        started.recv_timeout(Duration::from_secs(5))?,
        started.recv_timeout(Duration::from_secs(5))?,
    ];
    both.sort();
    assert_eq!(both, ["query.region.close", "scope.review"]);
    for (id, command) in [("3", "job.pause"), ("4", "job.resume"), ("5", "job.cancel")] {
        assert_eq!(
            requests.enqueue_ipc(&entry(), id, command),
            EnqueueResult::Accepted
        );
        assert_eq!(
            recv_message(&owner, Duration::from_secs(5))?,
            format!("{id}\n{command}")
        );
    }
    release_query.send(())?;
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "2\nquery.region.close"
    );
    release_review.send(())?;
    assert_eq!(
        recv_message(&owner, Duration::from_secs(5))?,
        "1\nscope.review"
    );
    Ok(())
}
