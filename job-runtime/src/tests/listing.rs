use super::*;

#[test]
fn list_filters_and_returns_submission_order_with_current_snapshots()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
    )?);
    runtime.register(descriptor())?;
    let entered = Arc::new(Barrier::new(2));
    let leave = Arc::new(Barrier::new(2));
    let first = created(runtime.submit(sample_submission(Arc::new(Body {
        entered: entered.clone(),
        leave: leave.clone(),
    }))?)?)?;
    let mut next = sample_submission(Arc::new(InstantBody))?;
    next.scope_summary = "second scope".into();
    next.priority = JobPriority::Low;
    let second = created(runtime.submit(next)?)?;
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    entered.wait();
    let rows = runtime.list(None)?;
    assert_eq!(
        rows.iter().map(|row| &row.job_id).collect::<Vec<_>>(),
        vec![&first, &second]
    );
    assert_eq!(rows[0].snapshot.state, JobState::Running);
    assert_eq!(rows[1].snapshot.state, JobState::Queued);
    assert_eq!(rows[0].wait_reason, JobWaitReason::Unknown);
    assert_eq!(rows[1].wait_reason, JobWaitReason::QueueFull);
    assert_eq!(rows[1].scope_summary, "second scope");
    assert_eq!(rows[1].priority, JobPriority::Low);
    assert_eq!(rows[1].supported_controls, vec![JobControl::Cancel]);
    for row in &rows {
        assert_eq!(row.snapshot, runtime.snapshot(&row.job_id)?);
    }
    assert_eq!(
        runtime.list(Some(&[JobState::Running]))?,
        vec![rows[0].clone()]
    );
    assert_eq!(
        runtime.list(Some(&[JobState::Queued, JobState::Running]))?,
        rows
    );
    assert!(runtime.list(Some(&[]))?.is_empty());
    leave.wait();
    worker.join().map_err(|_| "worker panicked")??;
    runtime.flush()?;
    let done = runtime.list(Some(&[JobState::Done]))?;
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].job_id, first);
    assert_eq!(done[0].wait_reason, JobWaitReason::None);
    let terminal = done[0]
        .snapshot
        .terminal
        .as_ref()
        .ok_or("terminal missing")?;
    assert_eq!(terminal.sequence, done[0].snapshot.sequence);
    assert_eq!(terminal.summary.job_id, first);
    Ok(())
}
