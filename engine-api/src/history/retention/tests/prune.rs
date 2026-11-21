use super::*;
use du_core_contracts::job::JobState;

fn unique_id(value: u64) -> Id {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&value.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Id::from_v7_bytes(bytes)
}

fn indexed_candidates(count: usize, reckon_at_ms: i64) -> Vec<HistoryExpiryCandidate> {
    (0..count)
        .map(|index| HistoryExpiryCandidate {
            job_id: unique_id(index as u64 + 1),
            has_nonterminal_follow_up: false,
            reckon_at_ms,
        })
        .collect()
}

fn run_one(setup: &Setup) -> Result<Id, String> {
    let id = (setup.trigger)().ok_or("history.prune was not submitted")?;
    let ran = setup
        .runtime
        .run_next()
        .map_err(|error| format!("run_next failed: {error:?}"))?;
    if ran.as_ref() != Some(&id) {
        return Err("history.prune was not the next job".into());
    }
    Ok(id)
}

fn summary(setup: &Setup, job_id: &Id) -> Result<du_core_contracts::job::JobSummary, String> {
    let flush = setup
        .runtime
        .begin_delivery_flush(std::time::Duration::from_secs(2))
        .map_err(|error| format!("delivery flush failed: {error:?}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while flush.poll() == du_job_runtime::ShutdownStatus::Pending
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    if flush.poll() != du_job_runtime::ShutdownStatus::Complete {
        return Err("job delivery did not flush".into());
    }
    setup
        .runtime
        .snapshot(job_id)
        .map_err(|error| format!("snapshot failed: {error:?}"))?
        .terminal
        .map(|terminal| terminal.summary)
        .ok_or_else(|| "history.prune did not finish".into())
}

#[test]
fn descriptor_matches_history_prune_contract() {
    let descriptor = super::super::prune::descriptor();
    assert_eq!(descriptor.kind_id, "history.prune");
    assert_eq!(descriptor.version, 1);
    assert_eq!(
        descriptor.controls,
        [du_core_contracts::job::JobControl::Cancel]
    );
    assert_eq!(descriptor.safe_points, ["batch_end"]);
    assert_eq!(
        descriptor.progress_unit,
        du_core_contracts::job::ProgressUnit::Items
    );
    assert_eq!(
        descriptor.resource_classes,
        [du_core_contracts::job::ResourceClass::Output]
    );
    assert_eq!(
        descriptor.history,
        du_core_contracts::job::HistoryMode::None
    );
}

#[test]
fn disabled_cleanup_does_not_submit_or_remove_history() -> Result<(), String> {
    let setup = setup(policy(false, None), vec![candidate(1, 1, false)])?;

    assert!((setup.trigger)().is_none());
    assert_eq!(
        setup.runtime.run_next().map_err(|e| format!("{e:?}"))?,
        None
    );
    assert_eq!(
        setup
            .reader
            .lock()
            .map_err(|e| e.to_string())?
            .candidates
            .len(),
        1
    );
    Ok(())
}

#[test]
fn queued_submissions_deduplicate_and_a_second_completed_prune_is_a_noop() -> Result<(), String> {
    let setup = setup(
        policy(true, Some(30)),
        vec![candidate(1, -3_000_000_000, false)],
    )?;
    let first = (setup.trigger)().ok_or("first submission was rejected")?;
    let duplicate = (setup.trigger)().ok_or("duplicate submission was rejected")?;
    assert_eq!(first, duplicate);
    assert_eq!(setup.wake_count.load(Ordering::SeqCst), 2);
    assert_eq!(
        setup
            .runtime
            .list(None)
            .map_err(|e| format!("{e:?}"))?
            .len(),
        1
    );

    setup
        .runtime
        .run_next()
        .map_err(|error| format!("run_next failed: {error:?}"))?;
    assert_eq!(setup.runtime.state(&first), Ok(JobState::Done));
    let generation = setup
        .reader
        .lock()
        .map_err(|error| error.to_string())?
        .delete_generation;
    assert_eq!(generation, 8);

    let second = run_one(&setup)?;
    assert_ne!(first, second);
    assert_eq!(setup.runtime.state(&second), Ok(JobState::Done));
    let state = setup.reader.lock().map_err(|error| error.to_string())?;
    assert!(state.candidates.is_empty());
    assert_eq!(state.delete_generation, generation);
    Ok(())
}

#[test]
fn mid_run_fingerprint_change_stops_before_the_next_batch() -> Result<(), String> {
    let setup = setup(
        policy(true, Some(30)),
        indexed_candidates(501, -3_000_000_000),
    )?;
    setup
        .reader
        .lock()
        .map_err(|error| error.to_string())?
        .apply_expiry_filter = true;
    setup
        .prune_delete
        .change_policy_after_delete
        .store(true, Ordering::SeqCst);
    let job_id = run_one(&setup)?;

    let summary = summary(&setup, &job_id)?;
    assert_eq!(summary.state, JobState::Done);
    assert_eq!(summary.counts.completed, 500);
    let state = setup.reader.lock().map_err(|error| error.to_string())?;
    assert_eq!(state.candidates.len(), 1);
    assert_eq!(state.delete_generation, 8);
    assert_eq!(state.prune_reads.len(), 1);
    assert!(
        state
            .prune_reads
            .iter()
            .all(|(_, reference, _, _)| *reference == 10_000)
    );
    Ok(())
}

#[test]
fn batch_failure_releases_undo_reservations_and_preserves_rows() -> Result<(), String> {
    let setup = setup(
        policy(true, Some(30)),
        vec![candidate(2, -3_000_000_000, false)],
    )?;
    setup
        .reader
        .lock()
        .map_err(|error| error.to_string())?
        .apply_expiry_filter = true;
    setup
        .undo
        .present
        .lock()
        .map_err(|error| error.to_string())?
        .insert(id(2));
    setup.prune_delete.fail.store(true, Ordering::SeqCst);
    let id = run_one(&setup)?;

    let summary = summary(&setup, &id)?;
    assert_eq!(summary.state, JobState::Failed);
    assert_eq!(
        summary.failure_reason_key.as_deref(),
        Some("history.storage_unavailable")
    );
    assert!(
        setup
            .undo
            .reserved
            .lock()
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    assert!(
        setup
            .undo
            .present
            .lock()
            .map_err(|error| error.to_string())?
            .contains(&super::id(2))
    );
    let state = setup.reader.lock().map_err(|error| error.to_string())?;
    assert_eq!(state.candidates.len(), 1);
    assert_eq!(state.delete_generation, 7);
    Ok(())
}

#[test]
fn undo_in_use_reservation_failure_and_follow_up_are_skipped() -> Result<(), String> {
    let candidates = vec![
        candidate(1, -3_000_000_000, false),
        candidate(2, -3_000_000_000, false),
        candidate(3, -3_000_000_000, false),
        candidate(4, -3_000_000_000, true),
        candidate(5, -3_000_000_000, false),
    ];
    let setup = setup(policy(true, Some(30)), candidates)?;
    setup
        .reader
        .lock()
        .map_err(|error| error.to_string())?
        .apply_expiry_filter = true;
    {
        let mut present = setup.undo.present.lock().map_err(|e| e.to_string())?;
        present.extend([id(2), id(3), id(5)]);
    }
    setup
        .undo
        .in_use
        .lock()
        .map_err(|error| error.to_string())?
        .insert(id(2));
    setup
        .undo
        .reserve_failures
        .lock()
        .map_err(|error| error.to_string())?
        .insert(id(3));
    setup
        .undo
        .in_use_errors
        .lock()
        .map_err(|error| error.to_string())?
        .insert(id(5));
    let job_id = run_one(&setup)?;

    let summary = summary(&setup, &job_id)?;
    assert_eq!(summary.state, JobState::Done);
    assert_eq!(summary.counts.completed, 1);
    assert_eq!(summary.counts.skipped, 4);
    let state = setup.reader.lock().map_err(|error| error.to_string())?;
    assert_eq!(state.candidates.len(), 4);
    assert_eq!(state.delete_generation, 8);
    assert!(
        setup
            .undo
            .reserved
            .lock()
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    Ok(())
}

#[test]
fn failure_after_a_committed_batch_finishes_partial() -> Result<(), String> {
    let setup = setup(
        policy(true, Some(30)),
        indexed_candidates(501, -3_000_000_000),
    )?;
    setup
        .reader
        .lock()
        .map_err(|error| error.to_string())?
        .apply_expiry_filter = true;
    setup
        .prune_delete
        .fail_after_first_batch
        .store(true, Ordering::SeqCst);
    let id = run_one(&setup)?;

    let summary = summary(&setup, &id)?;
    assert_eq!(summary.state, JobState::Partial);
    assert_eq!(summary.counts.completed, 500);
    assert_eq!(
        summary.failure_reason_key.as_deref(),
        Some("history.storage_unavailable")
    );
    assert_eq!(
        setup
            .reader
            .lock()
            .map_err(|e| e.to_string())?
            .candidates
            .len(),
        1
    );
    Ok(())
}

#[test]
fn cancel_is_honored_after_the_current_batch() -> Result<(), String> {
    let setup = setup(
        policy(true, Some(30)),
        indexed_candidates(501, -3_000_000_000),
    )?;
    setup
        .reader
        .lock()
        .map_err(|error| error.to_string())?
        .apply_expiry_filter = true;
    let job_id = (setup.trigger)().ok_or("history.prune was not submitted")?;
    *setup
        .prune_delete
        .cancel_after_delete
        .lock()
        .map_err(|error| error.to_string())? = Some((setup.runtime.clone(), job_id.clone()));
    setup
        .runtime
        .run_next()
        .map_err(|error| format!("run_next failed: {error:?}"))?;

    let summary = summary(&setup, &job_id)?;
    assert_eq!(summary.state, JobState::Cancelled);
    assert_eq!(summary.counts.completed, 500);
    let state = setup.reader.lock().map_err(|error| error.to_string())?;
    assert_eq!(state.candidates.len(), 1);
    assert_eq!(state.delete_generation, 8);
    Ok(())
}

#[test]
fn shortening_from_ninety_to_thirty_days_removes_a_sixty_day_history() -> Result<(), String> {
    let sixty_days_ms = 60_i64 * 86_400_000;
    let setup = setup(
        policy(true, Some(90)),
        vec![candidate(1, 10_000 - sixty_days_ms, false)],
    )?;
    setup
        .reader
        .lock()
        .map_err(|error| error.to_string())?
        .apply_expiry_filter = true;

    let reviewed = review(&setup, json!({"enabled":true,"days":30}))?;
    assert_eq!(reviewed["targetCount"], "1");
    let response = apply(
        &setup,
        &reviewed,
        reviewed["fingerprint"]
            .as_str()
            .ok_or("missing fingerprint")?,
    );
    assert!(response.error.is_none());
    assert!(response.body["cleanupJobRef"].is_string());
    let job_ref: Id = serde_json::from_value(response.body["cleanupJobRef"].clone())
        .map_err(|error| error.to_string())?;
    setup
        .runtime
        .run_next()
        .map_err(|error| format!("run_next failed: {error:?}"))?;

    let summary = summary(&setup, &job_ref)?;
    assert_eq!(summary.counts.completed, 1);
    assert!(
        setup
            .reader
            .lock()
            .map_err(|error| error.to_string())?
            .candidates
            .is_empty()
    );
    Ok(())
}
