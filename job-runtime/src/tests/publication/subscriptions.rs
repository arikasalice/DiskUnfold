use super::*;

#[test]
fn final_detach_cancels_and_closes_subscriber_admission() -> Result<(), Box<dyn std::error::Error>>
{
    let (runtime, id) = setup_kind("viz.compute", Arc::new(Events::default()))?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let committed = Arc::new(AtomicBool::new(false));
    let published = committed.clone();
    runtime.submit_viz_with_publication(
        controlled_submission("viz.compute", entered.clone(), release.clone(), id.clone()),
        move || {
            published.store(true, Ordering::SeqCst);
            Ok(())
        },
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    let mut active = 0;
    for _ in 0..2 {
        runtime.inspect_viz_gate(&id, |state| {
            assert_eq!(state, JobState::Running);
            active += 1;
        })?;
    }
    let mut detach = || {
        active -= 1;
        Ok(active == 0)
    };
    assert_eq!(runtime.cancel_viz_if(&id, &mut detach)?, None);
    assert_eq!(runtime.state(&id)?, JobState::Running);
    let (accepted, sequence) = runtime.cancel_viz_if(&id, detach)?.ok_or("last detach")?;
    assert_eq!(accepted, CancelResult::Accepted(JobState::CancelRequested));
    assert_eq!(
        runtime.inspect_viz_gate(&id, |state| state)?,
        JobState::CancelRequested
    );
    assert_eq!(runtime.snapshot(&id)?.sequence, sequence);
    assert_eq!(active, 0);
    release.wait();
    worker.join().map_err(|_| "worker panicked")??;
    assert_eq!(runtime.state(&id)?, JobState::Cancelled);
    assert!(!committed.load(Ordering::SeqCst));
    Ok(())
}

#[test]
fn commit_that_wins_the_gate_skips_late_detachment() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup_kind("viz.compute", Arc::new(Events::default()))?;
    let publishing = Arc::new(Barrier::new(2));
    let complete = Arc::new(Barrier::new(2));
    let started = publishing.clone();
    let finish = complete.clone();
    runtime.submit_viz_with_publication(
        controlled_submission(
            "viz.compute",
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        move || {
            started.wait();
            finish.wait();
            Ok(())
        },
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    publishing.wait();
    let detached = Arc::new(AtomicBool::new(false));
    let attempt = Arc::new(Barrier::new(2));
    let cancellation = {
        let runtime = runtime.clone();
        let id = id.clone();
        let detached = detached.clone();
        let attempt = attempt.clone();
        std::thread::spawn(move || {
            attempt.wait();
            runtime.cancel_viz_if(&id, || {
                detached.store(true, Ordering::SeqCst);
                Ok(true)
            })
        })
    };
    attempt.wait();
    complete.wait();
    worker.join().map_err(|_| "worker panicked")??;
    let (result, _) = cancellation
        .join()
        .map_err(|_| "cancel panicked")??
        .ok_or("terminal acknowledgement")?;
    assert_eq!(result, CancelResult::AlreadyTerminal(JobState::Done));
    assert!(!detached.load(Ordering::SeqCst));
    Ok(())
}

#[test]
fn query_job_cannot_be_used_for_visualization_subscriptions()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let result = id.clone();
    runtime.submit_with_publication(
        submission(
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        move || Ok(result),
    )?;
    assert_eq!(
        runtime.inspect_viz_gate(&id, |state| state),
        Err(RuntimeError::InvalidDescriptor)
    );
    assert_eq!(
        runtime.cancel_viz_if(&id, || Ok(true)),
        Err(RuntimeError::InvalidDescriptor)
    );
    assert_eq!(runtime.state(&id)?, JobState::Queued);
    Ok(())
}
