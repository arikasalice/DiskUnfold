use super::*;

#[test]
fn invalid_scope_or_duplicate_targets_fail_before_any_open() -> Result<(), String> {
    let store = store()?;
    let gate = gate();
    let mut wrong_scope = scope();
    wrong_scope.roots[0].result_version = Id::from_v7_bytes([8; 16]);
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        job::prepare(
            Input {
                source: source(),
                result: result_id(),
                scope: wrong_scope,
                targets: targets(),
                reader: Port(Arc::clone(&gate))
            },
            hash,
            Arc::clone(&store)
        ),
        Err(job::PrepareError::InvalidScope)
    ));
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let mut repeated = targets();
    repeated.push(repeated[0].clone());
    assert!(matches!(
        job::prepare(
            Input {
                source: source(),
                result: result_id(),
                scope: scope(),
                targets: repeated,
                reader: Port(Arc::clone(&gate))
            },
            hash,
            store
        ),
        Err(job::PrepareError::Verification(_))
    ));
    assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn disabled_content_flow_fails_without_open_or_publication() -> Result<(), String> {
    let runtime = JobRuntime::new(
        Arc::new(Identity),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events),
    )
    .map_err(|e| format!("{e:?}"))?;
    runtime
        .register(job::descriptor())
        .map_err(|e| format!("{e:?}"))?;
    let store = store()?;
    let gate = gate();
    let id = submit(&runtime, prepare(Arc::clone(&gate), Arc::clone(&store))?)?;
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let snapshot = runtime.snapshot(&id).map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(
        snapshot.failure_reason_key.as_deref(),
        Some("support.internal_error")
    );
    assert_eq!(snapshot.result_version_id, None);
    assert!(store.pin(Some(&result_id())).is_err());
    assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn empty_scope_finishes_without_content_capacity_or_reads() -> Result<(), String> {
    let runtime = JobRuntime::new(
        Arc::new(Identity),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events),
    )
    .map_err(|e| format!("{e:?}"))?;
    runtime
        .register(job::descriptor())
        .map_err(|e| format!("{e:?}"))?;
    let store = store()?;
    let gate = gate();
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let prepared = job::prepare(
        Input {
            source: source(),
            result: result_id(),
            scope: scope(),
            targets: vec![],
            reader: Port(Arc::clone(&gate)),
        },
        hash,
        Arc::clone(&store),
    )
    .map_err(|e| format!("{e:?}"))?;
    let id = submit(&runtime, prepared)?;
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        runtime.state(&id).map_err(|e| format!("{e:?}"))?,
        JobState::Done
    );
    assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
    assert!(
        store
            .pin(Some(&result_id()))
            .map_err(|e| format!("{e:?}"))?
            .outcome()
            .evidence()
            .groups
            .is_empty()
    );
    Ok(())
}

#[test]
fn cancellation_at_terminal_gate_marks_prepared_completion_cancelled() -> Result<(), String> {
    use du_core_contracts::job::{Checkpoint, JobFlow, JobFlowError};
    use du_job_runtime::DuplicateTerminal;
    struct Flow;
    impl JobFlow for Flow {
        fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
            Ok(())
        }
        fn finish(self: Box<Self>, _: JobCounts) {}
    }
    struct Context;
    impl JobContext for Context {
        fn checkpoint(&self) -> Checkpoint {
            Checkpoint::Continue
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Ok(Box::new(Flow))
        }
    }
    let store = store()?;
    let gate = gate();
    let prepared = prepare(Arc::clone(&gate), Arc::clone(&store))?;
    let release = Arc::clone(&gate);
    let worker = std::thread::spawn(move || {
        release.entered.wait();
        release.release.wait();
    });
    let outcome = prepared.body.run(Arc::new(Context));
    worker.join().map_err(|_| "release worker panicked")?;
    assert!(outcome.completed_scope);
    assert_eq!(
        outcome.unprocessed_summary.as_deref(),
        Some("0 selected items lack final disposition")
    );
    (prepared.commit)(DuplicateTerminal::Cancelled)?;
    let pinned = store
        .pin(Some(&result_id()))
        .map_err(|e| format!("{e:?}"))?;
    assert!(pinned.outcome().evidence().cancelled);
    assert_eq!(pinned.outcome().evidence().groups.len(), 1);
    Ok(())
}

#[test]
fn normal_traversal_publishes_unique_size_and_blocked_peer_separately() -> Result<(), String> {
    use du_duplicate_engine::{result_store::ContentCoverage, verification::DistinctStage};
    let runtime = setup()?;
    let store = store()?;
    let gate = gate();
    let mut selected = targets();
    selected[1].snapshot = None;
    let mut unique = targets()[0].clone();
    unique.target.item_no = 3;
    unique.target.size = Size::Exact(4);
    if let Some(snapshot) = &mut unique.snapshot {
        snapshot.size = 4;
        snapshot.identity.file = vec![3];
    }
    if let Some(identity) = &mut unique.target.identity {
        identity.file = vec![3];
    }
    selected.push(unique);
    let mut evidence = scope();
    evidence.unavailable = 0;
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let prepared = job::prepare(
        Input {
            source: source(),
            result: result_id(),
            scope: evidence,
            targets: selected,
            reader: Port(Arc::clone(&gate)),
        },
        hash,
        Arc::clone(&store),
    )
    .map_err(|e| format!("{e:?}"))?;
    let id = submit(&runtime, prepared)?;
    runtime.run_next().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        runtime.state(&id).map_err(|e| format!("{e:?}"))?,
        JobState::Done
    );
    let pinned = store
        .pin(Some(&result_id()))
        .map_err(|e| format!("{e:?}"))?;
    let outcome = pinned.outcome().evidence();
    assert_eq!(outcome.states[&1], State::Candidate);
    assert!(outcome.unjudged.is_empty());
    assert_eq!(outcome.states[&2], State::Unchecked);
    assert_eq!(outcome.states[&3], State::Distinct);
    assert_eq!(outcome.distinct_by[&3], DistinctStage::Size);
    assert_eq!(pinned.content_coverage(), ContentCoverage::Partial);
    assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn unique_size_stage_requires_complete_scope_and_unambiguous_peer_sizes() -> Result<(), String> {
    use du_duplicate_engine::{result_store::ContentCoverage, verification::DistinctStage};
    for (complete, uncertain_peer) in [(true, false), (false, false), (true, true)] {
        let runtime = setup()?;
        let store = store()?;
        let gate = gate();
        let mut selected = targets();
        selected[1].target.size = if uncertain_peer {
            Size::Unknown
        } else {
            Size::Exact(4)
        };
        if let Some(snapshot) = &mut selected[1].snapshot {
            snapshot.size = 4;
        }
        let mut evidence = scope();
        evidence.unavailable = 0;
        if !complete {
            evidence.source_completeness = ResultCompleteness::Partial;
        }
        let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
        let prepared = job::prepare(
            Input {
                source: source(),
                result: result_id(),
                scope: evidence,
                targets: selected,
                reader: Port(Arc::clone(&gate)),
            },
            hash,
            Arc::clone(&store),
        )
        .map_err(|e| format!("{e:?}"))?;
        let id = submit(&runtime, prepared)?;
        runtime.run_next().map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            runtime.state(&id).map_err(|e| format!("{e:?}"))?,
            JobState::Done
        );
        let pinned = store
            .pin(Some(&result_id()))
            .map_err(|e| format!("{e:?}"))?;
        let outcome = pinned.outcome().evidence();
        if complete && !uncertain_peer {
            assert_eq!(outcome.states[&1], State::Distinct);
            assert_eq!(outcome.distinct_by[&1], DistinctStage::Size);
            assert_eq!(pinned.content_coverage(), ContentCoverage::Complete);
        } else {
            assert_eq!(outcome.states[&1], State::Candidate);
            assert!(!outcome.distinct_by.contains_key(&1));
            assert_eq!(pinned.content_coverage(), ContentCoverage::Partial);
        }
        assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

#[test]
fn cancellation_after_terminal_gate_cannot_relabel_published_completion() -> Result<(), String> {
    let runtime = setup()?;
    let store = store()?;
    let file_gate = gate();
    let mut prepared = prepare(Arc::clone(&file_gate), Arc::clone(&store))?;
    let publish_enter = Arc::new(Barrier::new(2));
    let publish_release = Arc::new(Barrier::new(2));
    let enter = Arc::clone(&publish_enter);
    let release = Arc::clone(&publish_release);
    let commit = prepared.commit;
    prepared.commit = Box::new(move |terminal| {
        enter.wait();
        release.wait();
        commit(terminal)
    });
    let id = submit(&runtime, prepared)?;
    let worker_runtime = Arc::clone(&runtime);
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    file_gate.entered.wait();
    file_gate.release.wait();
    publish_enter.wait();
    let cancel_runtime = Arc::clone(&runtime);
    let cancel_id = id.clone();
    let cancel = std::thread::spawn(move || cancel_runtime.cancel(&cancel_id));
    publish_release.wait();
    worker
        .join()
        .map_err(|_| "worker panicked")?
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        cancel.join().map_err(|_| "cancel panicked")?,
        Ok(CancelResult::AlreadyTerminal(JobState::Done))
    ));
    assert_eq!(
        runtime.state(&id).map_err(|e| format!("{e:?}"))?,
        JobState::Done
    );
    assert!(
        !store
            .pin(Some(&result_id()))
            .map_err(|e| format!("{e:?}"))?
            .outcome()
            .evidence()
            .cancelled
    );
    Ok(())
}

#[test]
fn late_cancellation_moves_precomputed_unjudged_candidates_into_result() -> Result<(), String> {
    use du_core_contracts::job::{Checkpoint, JobFlow, JobFlowError};
    use du_duplicate_engine::result_store::ContentCoverage;
    use du_job_runtime::DuplicateTerminal;
    struct Flow;
    impl JobFlow for Flow {
        fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
            Ok(())
        }
        fn finish(self: Box<Self>, _: JobCounts) {}
    }
    struct Context;
    impl JobContext for Context {
        fn checkpoint(&self) -> Checkpoint {
            Checkpoint::Continue
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Ok(Box::new(Flow))
        }
    }
    let store = store()?;
    let gate = gate();
    let mut selected = targets();
    selected[1].snapshot = None;
    let mut evidence = scope();
    evidence.unavailable = 0;
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let prepared = job::prepare(
        Input {
            source: source(),
            result: result_id(),
            scope: evidence,
            targets: selected,
            reader: Port(Arc::clone(&gate)),
        },
        hash,
        Arc::clone(&store),
    )
    .map_err(|e| format!("{e:?}"))?;
    let outcome = prepared.body.run(Arc::new(Context));
    assert!(outcome.completed_scope);
    assert_eq!(outcome.counts.unprocessed, 1);
    (prepared.commit)(DuplicateTerminal::Cancelled)?;
    let pinned = store
        .pin(Some(&result_id()))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(pinned.content_coverage(), ContentCoverage::Cancelled);
    assert_eq!(pinned.outcome().evidence().unjudged, vec![1]);
    assert_eq!(pinned.outcome().evidence().states[&1], State::Candidate);
    assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
    Ok(())
}
