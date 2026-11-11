use super::*;

#[test]
fn runtime_publishes_first_and_reused_results_with_separate_history() -> Result<(), String> {
    let h = Harness::new(NonZeroUsize::MIN.saturating_add(4))?;
    assert_eq!(h.selection.files().len(), 2);
    let first_job = h.submit(h.prepare(id(10), None)?)?;
    assert_eq!(h.runtime.state(&first_job), Ok(JobState::Queued));
    h.runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let first = assert_result(&h, &first_job, 10, false, 6)?;
    let mut replacement = h.source.open(id(3)).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(id(4), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        select_whole_scan(&h.source, &id(2), 0, || false),
        Err(du_engine_api::duplicate_scope::Error::StaleVersion)
    ));
    assert_eq!(h.selection.version(), &id(2));
    let second_job = h.submit(h.prepare(id(11), None)?)?;
    h.runtime.run_next().map_err(|e| format!("{e:?}"))?;
    assert_result(&h, &second_job, 11, true, 0)?;
    assert_eq!(first.outcome().evidence().full_reads.observed, 6);
    assert_eq!(first.outcome().evidence().groups.len(), 1);
    let rows = h.history.wait(2)?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter().map(|r| r.state).collect::<Vec<_>>(),
        [JobState::Done, JobState::Done]
    );
    assert_eq!(rows[0].result_version_id, Some(id(10)));
    assert_eq!(rows[1].result_version_id, Some(id(11)));
    Ok(())
}

fn assert_result(
    h: &Harness,
    job: &Id,
    version: u8,
    reused: bool,
    bytes: u64,
) -> Result<Arc<du_duplicate_engine::result_store::PinnedResult>, String> {
    let state = h.runtime.snapshot(job).map_err(|e| format!("{e:?}"))?;
    assert_eq!(state.state, JobState::Done);
    assert_eq!(state.result_version_id, Some(id(version)));
    let result = h
        .store
        .pin(Some(&id(version)))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.source(), h.selection.version());
    assert_eq!(result.outcome().evidence().full_reads.observed, bytes);
    assert_eq!(h.stats.bytes.load(Ordering::SeqCst), 6);
    assert_eq!(result.outcome().evidence().groups.len(), 1);
    assert_eq!(result.outcome().evidence().full_fingerprints.len(), 2);
    assert!(
        result
            .outcome()
            .evidence()
            .full_fingerprints
            .values()
            .all(|v| v.reused == reused && v.verified_at_ms == Some(100))
    );
    Ok(result)
}

#[test]
fn queued_cancel_publishes_cancelled_result_without_opening_files() -> Result<(), String> {
    let h = Harness::new(NonZeroUsize::MIN.saturating_add(2))?;
    let job = h.submit(h.prepare(id(10), None)?)?;
    assert!(matches!(
        h.runtime.cancel(&job),
        Ok(CancelResult::Accepted(JobState::Cancelled))
    ));
    let snapshot = h.runtime.snapshot(&job).map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.state, JobState::Cancelled);
    assert_eq!(snapshot.result_version_id, Some(id(10)));
    let result = h.store.pin(Some(&id(10))).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.content_coverage(), ContentCoverage::Cancelled);
    assert!(result.outcome().evidence().cancelled);
    assert!(result.outcome().evidence().groups.is_empty());
    assert_eq!(h.stats.opens.load(Ordering::SeqCst), 0);
    let rows = h.history.wait(1)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, JobState::Cancelled);
    assert_eq!(rows[0].result_version_id, Some(id(10)));
    Ok(())
}

#[test]
fn item_read_failure_does_not_create_a_group_and_is_recorded() -> Result<(), String> {
    let h = Harness::new(NonZeroUsize::MIN.saturating_add(2))?;
    let failing = h.selection.files()[0].item.item_no;
    let job = h.submit(h.prepare(id(10), Some(failing))?)?;
    h.runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let snapshot = h.runtime.snapshot(&job).map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.state, JobState::Done);
    assert_eq!(snapshot.result_version_id, Some(id(10)));
    let result = h.store.pin(Some(&id(10))).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        result.outcome().evidence().states[&failing],
        State::ReadFailed
    );
    assert!(result.outcome().evidence().groups.is_empty());
    assert_eq!(result.content_coverage(), ContentCoverage::Partial);
    let rows = h.history.wait(1)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, JobState::Done);
    assert_eq!(rows[0].counts.failed, 1);
    Ok(())
}

#[test]
fn result_store_capacity_failure_preserves_prior_result_and_reports_partial_job()
-> Result<(), String> {
    let h = Harness::new(NonZeroUsize::MIN)?;
    let first = h.submit(h.prepare(id(10), None)?)?;
    h.runtime.run_next().map_err(|e| format!("{e:?}"))?;
    assert_eq!(h.runtime.state(&first), Ok(JobState::Done));
    let old = h.store.pin(Some(&id(10))).map_err(|e| format!("{e:?}"))?;
    let second = h.submit(h.prepare(id(11), None)?)?;
    h.runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let state = h.runtime.snapshot(&second).map_err(|e| format!("{e:?}"))?;
    assert_eq!(state.state, JobState::Partial);
    assert!(state.failure_reason_key.is_some());
    assert_eq!(state.result_version_id, None);
    assert!(h.store.pin(Some(&id(11))).is_err());
    assert_eq!(old.outcome().evidence().groups.len(), 1);
    let rows = h.history.wait(2)?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].state, JobState::Partial);
    assert_eq!(rows[1].result_version_id, None);
    Ok(())
}

#[test]
fn running_cancel_discards_inflight_hash_and_records_terminal_history() -> Result<(), String> {
    let h = Harness::new(NonZeroUsize::MIN.saturating_add(3))?;
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    *h.stats.gate.lock().map_err(|_| "gate lock")? = Some(ReadGate {
        entered: entered_tx,
        release: release_rx,
    });
    let job = h.submit(h.prepare(id(10), None)?)?;
    let runtime = Arc::clone(&h.runtime);
    let worker = std::thread::spawn(move || runtime.run_next());
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "read did not start")?;
    assert!(matches!(
        h.runtime.cancel(&job),
        Ok(CancelResult::Accepted(JobState::CancelRequested))
    ));
    release_tx.send(()).map_err(|_| "read release failed")?;
    worker
        .join()
        .map_err(|_| "worker panic")?
        .map_err(|e| format!("{e:?}"))?;
    let state = h.runtime.snapshot(&job).map_err(|e| format!("{e:?}"))?;
    assert_eq!(state.state, JobState::Cancelled);
    assert_eq!(state.result_version_id, Some(id(10)));
    let result = h.store.pin(Some(&id(10))).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.content_coverage(), ContentCoverage::Cancelled);
    assert!(result.outcome().evidence().groups.is_empty());
    assert!(!result.outcome().evidence().interrupted.is_empty());
    assert!(result.outcome().evidence().full_fingerprints.is_empty());
    assert_eq!(h.stats.bytes.load(Ordering::SeqCst), 3);
    let retry = h.submit(h.prepare(id(11), None)?)?;
    h.runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let retry_result = h.store.pin(Some(&id(11))).map_err(|e| format!("{e:?}"))?;
    assert_eq!(h.runtime.state(&retry), Ok(JobState::Done));
    assert_eq!(retry_result.outcome().evidence().full_reads.observed, 6);
    assert!(
        retry_result
            .outcome()
            .evidence()
            .full_fingerprints
            .values()
            .all(|v| !v.reused)
    );
    let rows = h.history.wait(2)?;
    assert_eq!(rows[0].state, JobState::Cancelled);
    assert_eq!(rows[0].result_version_id, Some(id(10)));
    assert_eq!(rows[1].state, JobState::Done);
    Ok(())
}

struct PlatformEvidence(bool);
impl du_engine_api::duplicate_assembly::Preflight for PlatformEvidence {
    fn observe(
        &mut self,
        _: &ItemRef,
        expected: &ContentIdentity,
    ) -> Option<(PlaceholderEvidence, ContentObservation)> {
        self.0.then(|| {
            (
                PlaceholderEvidence::Local,
                ContentObservation {
                    identity: expected.clone(),
                    size: 3,
                    modified: 7,
                    change_stamp: Some(9),
                },
            )
        })
    }
}
fn submit_assembled(h: &Harness, result: u8, supported: bool) -> Result<Id, String> {
    let selection =
        select_whole_scan(&h.source, &id(2), 0, || false).map_err(|e| format!("{e:?}"))?;
    let identities = selection
        .files()
        .iter()
        .map(|file| {
            selection
                .content_evidence(&file.item)
                .map(|e| (file.item.item_no, e.identity))
                .ok_or("evidence")
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let port = Port {
        selected: identities,
        stats: h.stats.clone(),
        fail: None,
    };
    let submitted = du_engine_api::duplicate_assembly::submit(
        &h.runtime,
        h.store.clone(),
        selection,
        (port, PlatformEvidence(supported)),
        du_engine_api::duplicate_assembly::Request {
            result: id(result),
            correlation: id(9),
            priority: JobPriority::Normal,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    match submitted {
        SubmitResult::Created(job) => Ok(job),
        other => Err(format!("{other:?}")),
    }
}
#[test]
fn assembled_supported_scope_pins_and_publishes() -> Result<(), String> {
    let h = Harness::new(NonZeroUsize::MIN.saturating_add(2))?;
    let job = submit_assembled(&h, 10, true)?;
    assert_eq!(h.stats.opens.load(Ordering::SeqCst), 0);
    let mut later = h.source.open(id(3)).map_err(|e| format!("{e:?}"))?;
    later.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    later
        .seal(id(4), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    h.runtime.run_next().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        h.runtime
            .snapshot(&job)
            .map_err(|e| format!("{e:?}"))?
            .result_version_id,
        Some(id(10))
    );
    let result = h.store.pin(Some(&id(10))).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.source(), &id(2));
    assert_eq!(result.outcome().evidence().groups.len(), 1);
    assert_eq!(h.stats.bytes.load(Ordering::SeqCst), 6);
    Ok(())
}
#[test]
fn assembled_unsupported_or_cancelled_opens_nothing() -> Result<(), String> {
    let h = Harness::new(NonZeroUsize::MIN.saturating_add(2))?;
    let job = submit_assembled(&h, 10, false)?;
    h.runtime.run_next().map_err(|e| format!("{e:?}"))?;
    let result = h.store.pin(Some(&id(10))).map_err(|e| format!("{e:?}"))?;
    assert_eq!(h.runtime.state(&job), Ok(JobState::Done));
    assert!(result.outcome().evidence().groups.is_empty());
    assert_eq!(h.stats.opens.load(Ordering::SeqCst), 0);
    let job = submit_assembled(&h, 11, true)?;
    assert!(matches!(
        h.runtime.cancel(&job),
        Ok(CancelResult::Accepted(JobState::Cancelled))
    ));
    let result = h.store.pin(Some(&id(11))).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.content_coverage(), ContentCoverage::Cancelled);
    assert_eq!(h.stats.opens.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn assembled_unregistered_runtime_rejects_without_content_open() -> Result<(), String> {
    let h = Harness::new(NonZeroUsize::MIN.saturating_add(2))?;
    let runtime = JobRuntime::new_with_content_io_limit(
        Arc::new(Ids(AtomicUsize::new(30))),
        Arc::new(Clock),
        h.history.clone(),
        Arc::new(Events),
        FlowLimits::disabled(),
        1,
    )
    .map_err(|e| format!("{e:?}"))?;
    let selection =
        select_whole_scan(&h.source, &id(2), 0, || false).map_err(|e| format!("{e:?}"))?;
    let port = Port {
        selected: BTreeMap::new(),
        stats: h.stats.clone(),
        fail: None,
    };
    let result = du_engine_api::duplicate_assembly::submit(
        &runtime,
        h.store.clone(),
        selection,
        (port, PlatformEvidence(true)),
        du_engine_api::duplicate_assembly::Request {
            result: id(10),
            correlation: id(9),
            priority: JobPriority::Normal,
        },
    );
    assert!(matches!(
        result,
        Err(du_engine_api::duplicate_assembly::Error::Runtime(
            du_job_runtime::RuntimeError::KindUnknown
        ))
    ));
    assert_eq!(h.stats.opens.load(Ordering::SeqCst), 0);
    assert!(h.store.pin(Some(&id(10))).is_err());
    Ok(())
}

#[test]
fn assembled_running_cancel_discards_inflight_result() -> Result<(), String> {
    let h = Harness::new(NonZeroUsize::MIN.saturating_add(2))?;
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    *h.stats.gate.lock().map_err(|_| "gate lock")? = Some(ReadGate {
        entered: entered_tx,
        release: release_rx,
    });
    let job = submit_assembled(&h, 10, true)?;
    let runtime = h.runtime.clone();
    let worker = std::thread::spawn(move || runtime.run_next());
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "read did not start")?;
    assert!(matches!(
        h.runtime.cancel(&job),
        Ok(CancelResult::Accepted(JobState::CancelRequested))
    ));
    release_tx.send(()).map_err(|_| "release")?;
    worker
        .join()
        .map_err(|_| "worker panic")?
        .map_err(|e| format!("{e:?}"))?;
    let result = h.store.pin(Some(&id(10))).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.content_coverage(), ContentCoverage::Cancelled);
    assert!(result.outcome().evidence().groups.is_empty());
    assert!(result.outcome().evidence().full_fingerprints.is_empty());
    Ok(())
}
