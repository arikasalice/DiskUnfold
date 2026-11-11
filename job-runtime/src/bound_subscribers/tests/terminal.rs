use super::*;

#[test]
fn finished_job_is_in_snapshot_and_not_replayed_after_rejoin()
-> Result<(), Box<dyn std::error::Error>> {
    let session = setup()?;
    let subscriber = session.subscribe().ok_or("subscriber")?;
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
    fake_overflow(&session);
    let worker = {
        let runtime = runtime.clone();
        thread::spawn(move || runtime.run_next())
    };
    let flow = rx.recv_timeout(Duration::from_secs(2))?;
    flow.finish(JobCounts::default());
    worker.join().map_err(|_| "worker panic")??;
    runtime.flush()?;
    let token = subscriber.begin_rejoin()?.ok_or("token")?;
    let cut = session.rejoin(&subscriber, token)?.ok_or("cut")?;
    let row = cut.get(&job).ok_or("job")?;
    assert_eq!(row.snapshot.state, JobState::Done);
    assert_eq!(row.history_recorded, Some(false));
    assert_eq!(row.history_warning, None);
    assert!(row.summary.is_some());
    assert!(subscriber.try_next(Duration::from_secs(1))?.is_none());
    Ok(())
}
