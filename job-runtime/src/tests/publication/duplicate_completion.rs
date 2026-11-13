use super::*;

fn setup_duplicate() -> Result<(Arc<JobRuntime>, Id), Box<dyn std::error::Error>> {
    let id = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Identity(id.clone())),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
    )?);
    let mut descriptor = descriptor();
    descriptor.kind_id = "duplicate.check".into();
    descriptor.resource_classes = vec![ResourceClass::ContentIo];
    runtime.register(descriptor)?;
    Ok((runtime, id))
}

fn queued_outcome() -> JobOutcome {
    JobOutcome {
        counts: JobCounts {
            unprocessed: 4,
            ..Default::default()
        },
        completed_scope: false,
        failure_reason_key: None,
        unprocessed_summary: Some("whole scope".into()),
        result_version_id: None,
    }
}

#[test]
fn queued_cancel_commits_prepared_partial_without_running_body()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup_duplicate()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let commit_calls = calls.clone();
    let result = id.clone();
    runtime.submit_duplicate_terminal(
        controlled_submission(
            "duplicate.check",
            Arc::new(Barrier::new(2)),
            Arc::new(Barrier::new(2)),
            id.clone(),
        ),
        queued_outcome(),
        move |state| {
            assert_eq!(state, DuplicateTerminal::Cancelled);
            commit_calls.fetch_add(1, Ordering::SeqCst);
            Ok(result)
        },
    )?;
    assert_eq!(
        runtime.cancel(&id)?,
        CancelResult::Accepted(JobState::Cancelled)
    );
    assert_eq!(runtime.run_next()?, None);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.snapshot(&id)?.result_version_id, Some(id.clone()));
    runtime.flush()?;
    let table = runtime.table.lock().map_err(|_| "table poisoned")?;
    let summary = table
        .jobs
        .get(&id)
        .and_then(|j| j.summary.as_ref())
        .ok_or("summary missing")?;
    assert_eq!(summary.counts.unprocessed, 4);
    assert_eq!(summary.unprocessed_summary.as_deref(), Some("whole scope"));
    Ok(())
}

#[test]
fn running_cancel_commits_after_body_and_reports_failure_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    for fail in [false, true] {
        let (runtime, id) = setup_duplicate()?;
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let result = id.clone();
        runtime.submit_duplicate_terminal(
            controlled_submission(
                "duplicate.check",
                entered.clone(),
                release.clone(),
                id.clone(),
            ),
            queued_outcome(),
            move |state| {
                assert_eq!(state, DuplicateTerminal::Cancelled);
                seen.fetch_add(1, Ordering::SeqCst);
                if fail {
                    Err("test.commit_failed".into())
                } else {
                    Ok(result)
                }
            },
        )?;
        let worker = {
            let runtime = runtime.clone();
            std::thread::spawn(move || runtime.run_next())
        };
        entered.wait();
        assert_eq!(
            runtime.cancel(&id)?,
            CancelResult::Accepted(JobState::CancelRequested)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        release.wait();
        worker.join().map_err(|_| "worker panic")??;
        let snapshot = runtime.snapshot(&id)?;
        assert_eq!(snapshot.state, JobState::Cancelled);
        assert_eq!(
            snapshot.result_version_id,
            if fail { None } else { Some(id) }
        );
        assert_eq!(
            snapshot.failure_reason_key.as_deref(),
            if fail {
                Some("test.commit_failed")
            } else {
                None
            }
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn terminal_completion_wins_gate_before_late_cancel() -> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup_duplicate()?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let committing = Arc::new(Barrier::new(2));
    let finish = Arc::new(Barrier::new(2));
    let commit_in = committing.clone();
    let commit_out = finish.clone();
    let result = id.clone();
    runtime.submit_duplicate_terminal(
        controlled_submission(
            "duplicate.check",
            entered.clone(),
            release.clone(),
            id.clone(),
        ),
        queued_outcome(),
        move |state| {
            assert_eq!(state, DuplicateTerminal::Completed);
            commit_in.wait();
            commit_out.wait();
            Ok(result)
        },
    )?;
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    release.wait();
    committing.wait();
    let cancel = {
        let runtime = runtime.clone();
        let id = id.clone();
        std::thread::spawn(move || runtime.cancel(&id))
    };
    finish.wait();
    worker.join().map_err(|_| "worker panic")??;
    assert_eq!(
        cancel.join().map_err(|_| "cancel panic")??,
        CancelResult::AlreadyTerminal(JobState::Done)
    );
    assert_eq!(runtime.snapshot(&id)?.result_version_id, Some(id));
    Ok(())
}

#[test]
fn queued_commit_failure_stays_cancelled_and_records_error()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup_duplicate()?;
    runtime.submit_duplicate_terminal(
        controlled_submission(
            "duplicate.check",
            Arc::new(Barrier::new(2)),
            Arc::new(Barrier::new(2)),
            id.clone(),
        ),
        queued_outcome(),
        |_| Err("test.commit_failed".into()),
    )?;
    assert_eq!(
        runtime.cancel(&id)?,
        CancelResult::Accepted(JobState::Cancelled)
    );
    let snapshot = runtime.snapshot(&id)?;
    assert_eq!(snapshot.state, JobState::Cancelled);
    assert_eq!(snapshot.result_version_id, None);
    assert_eq!(
        snapshot.failure_reason_key.as_deref(),
        Some("test.commit_failed")
    );
    Ok(())
}

#[test]
fn completed_publication_failure_reports_failure_without_result()
-> Result<(), Box<dyn std::error::Error>> {
    let (runtime, id) = setup_duplicate()?;
    runtime.submit_duplicate_terminal(
        controlled_submission(
            "duplicate.check",
            Arc::new(Barrier::new(1)),
            Arc::new(Barrier::new(1)),
            id.clone(),
        ),
        queued_outcome(),
        |terminal| {
            assert_eq!(terminal, DuplicateTerminal::Completed);
            Err("test.commit_failed".into())
        },
    )?;
    runtime.run_next()?;
    let snapshot = runtime.snapshot(&id)?;
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(snapshot.result_version_id, None);
    assert_eq!(
        snapshot.failure_reason_key.as_deref(),
        Some("test.commit_failed")
    );
    Ok(())
}
