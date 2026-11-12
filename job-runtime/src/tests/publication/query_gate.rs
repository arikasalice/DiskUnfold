use super::*;

#[test]
fn matching_query_cancellation_wins_before_publication() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let committed = Arc::new(AtomicBool::new(false));
    let published = committed.clone();
    let reference = id.clone();
    runtime.submit_with_publication(
        submission(entered.clone(), release.clone(), id.clone()),
        move || {
            published.store(true, Ordering::SeqCst);
            Ok(reference)
        },
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    assert_eq!(
        runtime.cancel_query_if(&id, |state| {
            assert_eq!(state, JobState::Running);
            Ok(false)
        })?,
        None
    );
    assert_eq!(runtime.state(&id)?, JobState::Running);
    let (result, _) = runtime
        .cancel_query_if(&id, |_| Ok(true))?
        .ok_or("missing cancellation")?;
    assert_eq!(result, CancelResult::Accepted(JobState::CancelRequested));
    release.wait();
    worker.join().map_err(|_| "worker panicked")??;
    assert_eq!(runtime.state(&id)?, JobState::Cancelled);
    assert_eq!(runtime.snapshot(&id)?.result_version_id, None);
    assert!(!committed.load(Ordering::SeqCst));
    Ok(())
}

#[test]
fn publication_winner_preserves_result_and_checks_terminal_binding()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let publishing = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let started = publishing.clone();
    let finish = release.clone();
    let reference = id.clone();
    runtime.submit_with_publication(
        submission(
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        move || {
            started.wait();
            finish.wait();
            Ok(reference)
        },
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    publishing.wait();
    let cancellation = {
        let runtime = runtime.clone();
        let id = id.clone();
        std::thread::spawn(move || {
            runtime.cancel_query_if(&id, |state| {
                assert_eq!(state, JobState::Done);
                Ok(true)
            })
        })
    };
    release.wait();
    worker.join().map_err(|_| "worker panicked")??;
    let (result, _) = cancellation
        .join()
        .map_err(|_| "cancel panicked")??
        .ok_or("missing terminal")?;
    assert_eq!(result, CancelResult::AlreadyTerminal(JobState::Done));
    assert_eq!(runtime.snapshot(&id)?.result_version_id, Some(id.clone()));
    assert_eq!(runtime.cancel_query_if(&id, |_| Ok(false))?, None);
    Ok(())
}

#[test]
fn query_gate_rejects_other_kind_and_contains_callback_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let reference = id.clone();
    runtime.submit_with_publication(
        submission(
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        move || Ok(reference),
    )?;
    assert_eq!(
        runtime.cancel_query_if(&id, |_| Err(RuntimeError::NotFound)),
        Err(RuntimeError::NotFound)
    );
    assert_eq!(
        runtime.cancel_query_if(&id, |_| std::panic::resume_unwind(Box::new("injected"))),
        Err(RuntimeError::InvalidState)
    );
    assert_eq!(runtime.state(&id)?, JobState::Queued);
    assert_eq!(runtime.run_next()?, Some(id.clone()));
    assert_eq!(runtime.state(&id)?, JobState::Done);

    let (other, other_id) = setup_kind("viz.compute", Arc::new(Events::default()))?;
    other.submit_viz_with_publication(
        controlled_submission(
            "viz.compute",
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            other_id.clone(),
        ),
        || Ok(()),
    )?;
    assert_eq!(
        other.cancel_query_if(&other_id, |_| Ok(true)),
        Err(RuntimeError::InvalidDescriptor)
    );
    Ok(())
}

#[test]
fn failed_publication_remains_failed_without_a_result() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    runtime.submit_with_publication(
        submission(
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        || Err("result.requery_required".into()),
    )?;
    assert_eq!(runtime.run_next()?, Some(id.clone()));
    let snapshot = runtime.snapshot(&id)?;
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(snapshot.result_version_id, None);
    assert_eq!(
        snapshot.failure_reason_key.as_deref(),
        Some("result.requery_required")
    );
    let (result, _) = runtime
        .cancel_query_if(&id, |state| {
            assert_eq!(state, JobState::Failed);
            Ok(true)
        })?
        .ok_or("missing terminal")?;
    assert_eq!(result, CancelResult::AlreadyTerminal(JobState::Failed));
    Ok(())
}

#[test]
fn matching_queued_query_cancels_without_running_or_publishing()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let committed = Arc::new(AtomicBool::new(false));
    let published = committed.clone();
    let reference = id.clone();
    runtime.submit_with_publication(
        submission(
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        move || {
            published.store(true, Ordering::SeqCst);
            Ok(reference)
        },
    )?;
    let (result, _) = runtime
        .cancel_query_if(&id, |_| Ok(true))?
        .ok_or("missing cancellation")?;
    assert_eq!(result, CancelResult::Accepted(JobState::Cancelled));
    assert_eq!(runtime.run_next()?, None);
    assert_eq!(runtime.snapshot(&id)?.result_version_id, None);
    assert!(!committed.load(Ordering::SeqCst));
    Ok(())
}

#[test]
fn region_stop_and_runtime_cancel_share_the_publication_gate()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let body_entered = Arc::new(Barrier::new(2));
    let body_release = Arc::new(Barrier::new(2));
    let region_stopped = Arc::new(Barrier::new(2));
    let cancel_release = Arc::new(Barrier::new(2));
    let committed = Arc::new(AtomicBool::new(false));
    let published = committed.clone();
    let reference = id.clone();
    runtime.submit_with_publication(
        submission(body_entered.clone(), body_release.clone(), id.clone()),
        move || {
            published.store(true, Ordering::SeqCst);
            Ok(reference)
        },
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    body_entered.wait();
    let cancellation = {
        let runtime = runtime.clone();
        let id = id.clone();
        let region_stopped = region_stopped.clone();
        let cancel_release = cancel_release.clone();
        std::thread::spawn(move || {
            runtime.cancel_query_if(&id, |_| {
                region_stopped.wait();
                cancel_release.wait();
                Ok(true)
            })
        })
    };
    region_stopped.wait();
    assert!(matches!(
        runtime.table.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
    body_release.wait();
    assert!(!committed.load(Ordering::SeqCst));
    cancel_release.wait();
    let (result, _) = cancellation
        .join()
        .map_err(|_| "cancel panicked")??
        .ok_or("missing cancellation")?;
    assert_eq!(result, CancelResult::Accepted(JobState::CancelRequested));
    worker.join().map_err(|_| "worker panicked")??;
    assert_eq!(runtime.state(&id)?, JobState::Cancelled);
    assert!(!committed.load(Ordering::SeqCst));
    Ok(())
}

#[test]
fn delivery_failure_after_stop_leaves_cancel_intent_and_terminal_recoverable()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let published = Arc::new(AtomicBool::new(false));
    let commit = published.clone();
    let reference = id.clone();
    runtime.submit_with_publication(
        submission(entered.clone(), release.clone(), id.clone()),
        move || {
            commit.store(true, Ordering::SeqCst);
            Ok(reference)
        },
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    let sender = runtime.table.lock().map_err(|_| "poisoned")?.sender.take();
    let stopped = AtomicBool::new(false);
    assert_eq!(
        runtime.cancel_query_if(&id, |_| {
            stopped.store(true, Ordering::SeqCst);
            Ok(true)
        }),
        Err(RuntimeError::DeliveryUnavailable)
    );
    assert!(stopped.load(Ordering::SeqCst));
    assert_eq!(runtime.state(&id)?, JobState::CancelRequested);
    runtime.table.lock().map_err(|_| "poisoned")?.sender = sender;
    release.wait();
    worker.join().map_err(|_| "worker panicked")??;
    assert_eq!(runtime.snapshot(&id)?.state, JobState::Cancelled);
    assert_eq!(runtime.snapshot(&id)?.result_version_id, None);
    assert!(!published.load(Ordering::SeqCst));
    Ok(())
}

#[test]
fn poisoned_runtime_rejects_before_region_change_and_cannot_report()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    runtime.submit_with_publication(
        submission(
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        {
            let id = id.clone();
            move || Ok(id)
        },
    )?;
    let poisoned = runtime.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poisoned.table.lock().unwrap_or_else(|e| e.into_inner());
        std::panic::resume_unwind(Box::new("injected table poison"));
    })
    .join();
    let stopped = AtomicBool::new(false);
    assert_eq!(
        runtime.cancel_query_if(&id, |_| {
            stopped.store(true, Ordering::SeqCst);
            Ok(true)
        }),
        Err(RuntimeError::Poisoned)
    );
    assert!(!stopped.load(Ordering::SeqCst));
    assert_eq!(runtime.state(&id), Err(RuntimeError::Poisoned));
    assert_eq!(runtime.run_next(), Err(RuntimeError::Poisoned));
    Ok(())
}

#[test]
fn poison_after_cancel_blocks_publication_but_prevents_terminal_report()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let published = Arc::new(AtomicBool::new(false));
    let commit = published.clone();
    let reference = id.clone();
    runtime.submit_with_publication(
        submission(entered.clone(), release.clone(), id.clone()),
        move || {
            commit.store(true, Ordering::SeqCst);
            Ok(reference)
        },
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    assert!(matches!(
        runtime.cancel_query_if(&id, |_| Ok(true))?,
        Some((CancelResult::Accepted(JobState::CancelRequested), _))
    ));
    let poisoned = runtime.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poisoned.table.lock().unwrap_or_else(|e| e.into_inner());
        std::panic::resume_unwind(Box::new("injected table poison"));
    })
    .join();
    release.wait();
    assert_eq!(
        worker.join().map_err(|_| "worker panicked")?,
        Err(RuntimeError::Poisoned)
    );
    assert!(!published.load(Ordering::SeqCst));
    assert_eq!(runtime.state(&id), Err(RuntimeError::Poisoned));
    Ok(())
}

#[test]
fn queued_delivery_failure_retains_cancelled_snapshot() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    runtime.submit_with_publication(
        submission(
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        {
            let id = id.clone();
            move || Ok(id)
        },
    )?;
    let sender = runtime.table.lock().map_err(|_| "poisoned")?.sender.take();
    assert_eq!(
        runtime.cancel_query_if(&id, |_| Ok(true)),
        Err(RuntimeError::DeliveryUnavailable)
    );
    assert_eq!(runtime.snapshot(&id)?.state, JobState::Cancelled);
    assert_eq!(runtime.snapshot(&id)?.result_version_id, None);
    runtime.table.lock().map_err(|_| "poisoned")?.sender = sender;
    assert_eq!(runtime.run_next()?, None);
    Ok(())
}

#[test]
fn unsupported_cancel_never_calls_region() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup()?;
    runtime.submit_with_publication(
        submission(
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        {
            let id = id.clone();
            move || Ok(id)
        },
    )?;
    runtime
        .table
        .lock()
        .map_err(|_| "poisoned")?
        .jobs
        .get_mut(&id)
        .ok_or("missing")?
        .descriptor
        .controls
        .clear();
    let called = AtomicBool::new(false);
    assert_eq!(
        runtime.cancel_query_if(&id, |_| {
            called.store(true, Ordering::SeqCst);
            Ok(true)
        }),
        Err(RuntimeError::InvalidDescriptor)
    );
    assert!(!called.load(Ordering::SeqCst));
    assert_eq!(runtime.state(&id)?, JobState::Queued);
    Ok(())
}
