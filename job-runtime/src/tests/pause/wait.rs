use super::*;

fn wait_until_parked(runtime: &JobRuntime) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while runtime.flow_limits.waiters.load(Ordering::Acquire) == 0 {
        if Instant::now() >= deadline {
            return Err("waiter did not park".into());
        }
        std::thread::yield_now();
    }
    Ok(())
}

fn spawn_waiter(
    context: Arc<dyn JobContext>,
) -> (
    std::thread::JoinHandle<()>,
    mpsc::Receiver<Result<(), JobFlowError>>,
) {
    let (send, recv) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let result = context
            .wait_start_flow()
            .map(|flow| flow.finish(JobCounts::default()));
        let _ = send.send(result);
    });
    (thread, recv)
}

#[test]
fn permit_release_wakes_waiter_and_preserves_limit() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, _, _, context, release, worker) =
        setup_with_resource(true, ResourceClass::ContentIo, 1)?;
    let held = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    let (waiter, result) = spawn_waiter(context.clone());
    wait_until_parked(&runtime)?;
    assert!(matches!(context.try_start_flow(), Err(JobFlowError::Busy)));
    held.finish(JobCounts::default());
    assert_eq!(result.recv_timeout(Duration::from_secs(2))?, Ok(()));
    waiter.join().map_err(|_| "waiter panic")?;
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    Ok(())
}

#[test]
fn pause_resume_and_cancel_wake_waiters() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, _, id, context, release, worker) =
        setup_with_resource(true, ResourceClass::ContentIo, 1)?;
    runtime.pause(&id)?;
    assert_eq!(runtime.state(&id)?, JobState::Paused);
    let (waiter, result) = spawn_waiter(context.clone());
    wait_until_parked(&runtime)?;
    runtime.resume(&id)?;
    assert_eq!(result.recv_timeout(Duration::from_secs(2))?, Ok(()));
    waiter.join().map_err(|_| "waiter panic")?;
    let held = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    let (waiter, result) = spawn_waiter(context.clone());
    wait_until_parked(&runtime)?;
    runtime.cancel(&id)?;
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2))?,
        Err(JobFlowError::Cancelled)
    );
    waiter.join().map_err(|_| "waiter panic")?;
    held.finish(JobCounts::default());
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    Ok(())
}

#[test]
fn abort_and_terminal_close_waiters() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, _, _, context, release, worker) =
        setup_with_resource(true, ResourceClass::ContentIo, 1)?;
    let held = context.try_start_flow().map_err(|e| format!("{e:?}"))?;
    let (waiter, result) = spawn_waiter(context.clone());
    wait_until_parked(&runtime)?;
    context.abort_flows();
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2))?,
        Err(JobFlowError::Closed)
    );
    waiter.join().map_err(|_| "waiter panic")?;
    held.finish(JobCounts::default());
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    assert!(matches!(
        context.wait_start_flow(),
        Err(JobFlowError::Closed)
    ));
    Ok(())
}

#[test]
fn disabled_resource_does_not_wait() -> Result<(), Box<dyn std::error::Error>> {
    let (_, _, _, context, release, worker) =
        setup_with_resource(true, ResourceClass::ContentIo, 0)?;
    assert!(matches!(
        context.wait_start_flow(),
        Err(JobFlowError::UnsupportedResource)
    ));
    release.send(())?;
    worker.join().map_err(|_| "worker panic")??;
    Ok(())
}

#[test]
fn body_return_closes_waiting_admission() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, _, id, context, release, worker) =
        setup_with_resource(true, ResourceClass::ContentIo, 1)?;
    runtime.pause(&id)?;
    let (waiter, result) = spawn_waiter(context);
    wait_until_parked(&runtime)?;
    release.send(())?;
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2))?,
        Err(JobFlowError::Closed)
    );
    waiter.join().map_err(|_| "waiter panic")?;
    worker.join().map_err(|_| "worker panic")??;
    Ok(())
}
