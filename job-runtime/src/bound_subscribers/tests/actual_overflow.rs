use super::*;

#[test]
fn real_runtime_isolates_slow_subscriber_and_rejoins_at_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let session = setup()?;
    let slow = session.subscribe().ok_or("slow")?;
    let fast = session.subscribe().ok_or("fast")?;
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
    runtime.flush()?;
    while slow.try_next(Duration::from_millis(100))?.is_some() {}
    while fast.try_next(Duration::from_millis(100))?.is_some() {}
    for _ in 0..1023 {
        flow.report_item(b"x").map_err(|_| "report")?;
        runtime.flush()?;
        assert!(matches!(
            fast.try_next(Duration::from_millis(100))?,
            Some(SubscriberMessage::Event(JobEvent::ItemResult { .. }))
        ));
    }
    assert!(slow.try_next(Duration::from_millis(99))?.is_none());
    flow.report_item(b"overflow")
        .map_err(|_| "overflow report")?;
    runtime.flush()?;
    assert!(matches!(
        slow.try_next(Duration::from_millis(100))?,
        Some(SubscriberMessage::ResyncRequired { dropped: 1024 })
    ));
    assert!(matches!(
        fast.try_next(Duration::from_millis(100))?,
        Some(SubscriberMessage::Event(JobEvent::ItemResult { .. }))
    ));
    flow.report_item(b"detached")
        .map_err(|_| "detached report")?;
    runtime.flush()?;
    assert!(slow.try_next(Duration::from_secs(1))?.is_none());
    let token = slow.begin_rejoin()?.ok_or("token")?;
    let cut = session.rejoin(&slow, token)?.ok_or("cut")?;
    let sequence = cut.get(&job).ok_or("job")?.snapshot.sequence;
    flow.report_item(b"after").map_err(|_| "after report")?;
    runtime.flush()?;
    assert!(
        matches!(slow.try_next(Duration::from_millis(100))?, Some(SubscriberMessage::Event(JobEvent::ItemResult { sequence: next, .. })) if next > sequence)
    );
    flow.finish(JobCounts::default());
    worker.join().map_err(|_| "worker panic")??;
    runtime.flush()?;
    Ok(())
}
