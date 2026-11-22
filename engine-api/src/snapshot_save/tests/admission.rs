use super::*;

#[test]
fn admission_stops_for_sample_incomplete_and_missing_lineage()
-> Result<(), Box<dyn std::error::Error>> {
    for (number, sample, lineage, terminal, expected) in [
        (10, true, true, true, "snapshot.sample_source"),
        (20, false, true, false, "snapshot.result_not_complete"),
        (30, false, false, true, "support.internal_error"),
    ] {
        let (store, version) = result_store(number, sample, lineage, terminal)?;
        let files = MemoryFiles::default();
        let index = MemoryIndex::default();
        let rig = rig(store, index.clone(), files.clone(), true)?;
        let error = rig
            .command
            .execute(request(version, true), &caller()?)
            .err()
            .ok_or("expected rejection")?;
        assert_eq!(reason(error), expected, "fixture {number}");
        assert_eq!(files.state().capacity_queries, 0);
        assert_eq!(files.state().create_calls, 0);
        assert_eq!(index.0.lock().map_err(|_| "index poisoned")?.lookups, 0);
    }
    Ok(())
}

#[test]
fn partial_terminal_result_is_admitted_with_unknown_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, version) = partial_result_store(35)?;
    let files = MemoryFiles::default();
    let partial_rig = rig(store, MemoryIndex::default(), files.clone(), true)?;
    assert!(matches!(
        partial_rig
            .command
            .execute(request(version, false), &caller()?)
            .map_err(reason)?,
        SnapshotSaveResponse::Accepted(_)
    ));
    assert_eq!(files.state().capacity_queries, 1);
    assert_eq!(files.state().create_calls, 0);
    assert_eq!(files.state().temporary_count, 0);
    assert_eq!(partial_rig.runtime.active_job_counts()?.total, 1);
    Ok(())
}

#[test]
fn missing_and_unconfirmed_versions_have_distinct_reasons() -> Result<(), Box<dyn std::error::Error>>
{
    let (store, version) = result_store(37, false, true, false)?;
    let files = MemoryFiles::default();
    let incomplete_rig = rig(store, MemoryIndex::default(), files.clone(), true)?;
    let incomplete = incomplete_rig
        .command
        .execute(request(version, false), &caller()?)
        .err()
        .ok_or("expected incomplete result rejection")?;
    assert_eq!(reason(incomplete), "snapshot.result_not_complete");
    assert_eq!(files.state().capacity_queries, 0);
    assert_eq!(files.state().create_calls, 0);

    let (store, _) = result_store(38, false, true, true)?;
    let files = MemoryFiles::default();
    let missing_rig = rig(store, MemoryIndex::default(), files.clone(), true)?;
    let missing = missing_rig
        .command
        .execute(request(id(9999)?, false), &caller()?)
        .err()
        .ok_or("expected missing result rejection")?;
    assert_eq!(reason(missing), "result.version_not_found");
    assert_eq!(files.state().capacity_queries, 0);
    assert_eq!(files.state().create_calls, 0);
    Ok(())
}

#[test]
fn snapshot_index_errors_keep_their_admission_reasons() {
    for (error, expected) in [
        (SnapshotIndexError::InvalidInput, "support.internal_error"),
        (SnapshotIndexError::Corrupt, "support.internal_error"),
        (SnapshotIndexError::UnsupportedSchema, "db.schema_mismatch"),
        (SnapshotIndexError::Busy, "db.busy"),
        (SnapshotIndexError::Storage, "snapshot.store_unavailable"),
    ] {
        assert_eq!(
            reason(super::super::errors::index_error(error)),
            expected,
            "{error:?}"
        );
    }
}

#[test]
fn existing_snapshot_and_running_job_precede_space_and_capability_checks()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, version) = result_store(40, false, true, true)?;
    let files = MemoryFiles::default();
    let index = MemoryIndex::default();
    let existing_id = id(500)?;
    index.0.lock().map_err(|_| "index poisoned")?.active = Some(existing_id.clone());
    let existing_rig = rig(store.clone(), index, files.clone(), true)?;
    let existing = existing_rig
        .command
        .execute(request(version.clone(), true), &caller()?)
        .map_err(reason)?;
    assert_eq!(existing, existing_snapshot(existing_id));
    assert_eq!(serde_json::to_value(&existing)?["pinApplied"], false);
    assert_eq!(files.state().capacity_queries, 0);
    assert_eq!(files.state().create_calls, 0);

    let files = MemoryFiles::default();
    let running_rig = rig(store, MemoryIndex::default(), files.clone(), true)?;
    let job_id = match running_rig.runtime.submit(no_op_submission(&version))? {
        SubmitResult::Created(id) => id,
        SubmitResult::Existing(_) => return Err("unexpected duplicate placeholder".into()),
    };
    let existing = running_rig
        .command
        .execute(request(version, true), &caller()?)
        .map_err(reason)?;
    assert_eq!(existing, existing_job(job_id));
    assert_eq!(serde_json::to_value(&existing)?["pinApplied"], false);
    assert_eq!(files.state().capacity_queries, 0);
    assert_eq!(files.state().create_calls, 0);
    Ok(())
}

#[test]
fn insufficient_space_and_unsupported_registration_stop_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, version) = result_store(50, false, true, true)?;
    let files = MemoryFiles::default();
    files.configure(|state| {
        state.available = Some(0);
        state.unsupported = true;
    });
    let low_space_rig = rig(store, MemoryIndex::default(), files.clone(), true)?;
    let error = low_space_rig
        .command
        .execute(request(version, false), &caller()?)
        .err()
        .ok_or("expected insufficient space")?;
    assert_eq!(error.internal_reason(), Some("snapshot.insufficient_space"));
    assert_eq!(reason(error), "snapshot.store_unavailable");
    assert_eq!(files.state().capacity_queries, 1);
    assert_eq!(files.state().create_calls, 0);
    assert_eq!(files.state().temporary_count, 0);
    assert_eq!(files.state().write_calls, 0);
    assert!(files.state().finals.is_empty());
    assert_eq!(low_space_rig.runtime.active_job_counts()?.total, 0);

    let (store, version) = result_store(60, false, true, true)?;
    let files = MemoryFiles::default();
    files.configure(|state| {
        state.available = Some(u64::MAX);
        state.unsupported = true;
    });
    let unsupported_rig = rig(store, MemoryIndex::default(), files.clone(), true)?;
    let error = unsupported_rig
        .command
        .execute(request(version, false), &caller()?)
        .err()
        .ok_or("expected unsupported capability")?;
    assert_eq!(reason(error), "snapshot.store_unavailable");
    assert_eq!(files.state().capacity_queries, 1);
    assert_eq!(files.state().create_calls, 0);
    assert_eq!(files.state().temporary_count, 0);
    assert_eq!(files.state().write_calls, 0);
    Ok(())
}

#[test]
fn non_main_window_is_denied_before_any_storage_access() -> Result<(), Box<dyn std::error::Error>> {
    let (store, version) = result_store(110, false, true, true)?;
    let files = MemoryFiles::default();
    let rig = rig(store, MemoryIndex::default(), files.clone(), false)?;
    let request = request(version, true);
    assert_eq!(
        reason(
            rig.command
                .check_scope(&request, &caller()?)
                .err()
                .ok_or("expected denial")?
        ),
        "command.window_denied"
    );
    assert_eq!(files.state().capacity_queries, 0);
    assert_eq!(files.state().create_calls, 0);
    Ok(())
}
