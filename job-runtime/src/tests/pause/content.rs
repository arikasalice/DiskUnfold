use super::*;

#[test]
fn content_flows_obey_budget_pause_resume_and_cancel() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, _, id, context, release, worker) =
        setup_with_resource(true, ResourceClass::ContentIo, 2)?;
    let first = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    let second = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    assert!(matches!(context.try_start_flow(), Err(JobFlowError::Busy)));
    runtime.pause(&id)?;
    let (thread, result) = start(first);
    assert_eq!(runtime.state(&id)?, JobState::PauseRequested);
    let (other_thread, other_result) = start(second);
    wait_state(&runtime, &id, JobState::Paused)?;
    runtime.resume(&id)?;
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2))?,
        Checkpoint::Continue
    );
    assert_eq!(
        other_result.recv_timeout(Duration::from_secs(2))?,
        Checkpoint::Continue
    );
    thread.join().map_err(|_| "thread panic")?;
    other_thread.join().map_err(|_| "thread panic")?;
    let inflight = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    runtime.cancel(&id)?;
    assert!(matches!(
        context.try_start_flow(),
        Err(JobFlowError::Cancelled)
    ));
    release.send(())?;
    assert_eq!(runtime.state(&id)?, JobState::CancelRequested);
    assert_eq!(inflight.safe_point(), Checkpoint::Cancel);
    inflight.finish(JobCounts::default());
    worker.join().map_err(|_| "worker panic")??;
    assert_eq!(runtime.state(&id)?, JobState::Cancelled);
    assert_eq!(
        runtime.flow_limits.content_active.load(Ordering::Acquire),
        0
    );
    Ok(())
}

#[test]
fn content_flows_require_explicit_nonzero_budget() -> Result<(), Box<dyn std::error::Error>> {
    let (_, _, _, context, release, worker) =
        setup_with_resource(true, ResourceClass::ContentIo, 0)?;
    assert!(matches!(
        context.try_start_flow(),
        Err(JobFlowError::UnsupportedResource)
    ));
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    Ok(())
}

#[test]
fn content_budget_is_shared_across_jobs_and_released_on_finish()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let runtime = Arc::new(JobRuntime::new_with_content_io_limit(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
        FlowLimits::disabled(),
        1,
    )?);
    let mut kind = descriptor();
    kind.resource_classes = vec![ResourceClass::ContentIo];
    runtime.register(kind)?;
    let (send, recv) = mpsc::channel();
    let mut releases = Vec::new();
    for _ in 0..2 {
        let (release, wait) = mpsc::channel();
        releases.push(release);
        runtime.submit(sample_submission(Arc::new(HoldingBody {
            send: send.clone(),
            release: Mutex::new(wait),
            completed_scope: true,
        }))?)?;
    }
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let runtime = runtime.clone();
            std::thread::spawn(move || runtime.run_next())
        })
        .collect();
    let first = recv.recv_timeout(Duration::from_secs(2))?;
    let second = recv.recv_timeout(Duration::from_secs(2))?;
    let flow = first.try_start_flow().map_err(|e| format!("{e:?}"))?;
    assert!(matches!(second.try_start_flow(), Err(JobFlowError::Busy)));
    flow.finish(JobCounts::default());
    second
        .try_start_flow()
        .map_err(|e| format!("{e:?}"))?
        .finish(JobCounts::default());
    for release in releases {
        release.send(())?;
    }
    for worker in workers {
        worker.join().map_err(|_| "worker panic")??;
    }
    assert_eq!(
        runtime.flow_limits.content_active.load(Ordering::Acquire),
        0
    );
    Ok(())
}
