use super::*;

fn verify_saved_row(
    state: &IndexState,
    version: &Id,
    files: &MemoryFiles,
) -> Result<Id, Box<dyn std::error::Error>> {
    let row = state.rows.first().ok_or("index row missing")?;
    let snapshot_id = row.snapshot_id.clone();
    assert_eq!(row.source_version_id, *version);
    assert_eq!(row.observed_at_ms, Some(1_600_000_000_000));
    assert_eq!(row.source_kind, SnapshotIndexSourceKind::Local);
    assert_eq!(row.condition_summary, "{\"state\":\"unknown\"}");
    assert_eq!(row.scope_label, Vec::<u8>::new());
    assert_eq!(row.completeness, Some(SnapshotIndexCompleteness::Complete));
    assert_eq!(
        row.format_version,
        du_snapshot_store::envelope::DUSNAP_FORMAT_VERSION
    );
    assert_eq!(row.file_name, format!("{}.dusnap", snapshot_id.as_str()));
    let bytes = &files.state().finals[&row.file_name];
    assert_eq!(row.body_size as usize, bytes.len());
    assert_eq!(
        row.body_hash,
        *blake3::hash(&bytes[..bytes.len() - 32]).as_bytes()
    );
    assert_eq!(state.baseline_at, Some(row.created_at_ms));
    Ok(snapshot_id)
}

fn verify_saved_volume(state: &IndexState) {
    assert_eq!(state.volumes.len(), 1);
    assert_eq!(state.volumes[0].observed_volume_id, Some(vec![1, 2, 3]));
    assert_eq!(
        state.volumes[0].identity_kind,
        SnapshotVolumeIdentityKind::None
    );
    assert_eq!(
        state.volumes[0].identity_capability,
        SnapshotVolumeIdentityCapability::Unknown
    );
}

fn verify_saved_event(
    events: &DesktopEvents,
    snapshot_id: &Id,
) -> Result<(), Box<dyn std::error::Error>> {
    std::thread::sleep(Duration::from_millis(110));
    let event: serde_json::Value =
        serde_json::from_str(&events.next_json().ok_or("snapshot event missing")?)?;
    assert_eq!(event["topic"], "snapshot.changed");
    assert_eq!(event["body"]["change"], "saved");
    assert_eq!(event["body"]["listSeq"], 1);
    assert!(event["body"]["observedAt"].is_string());
    assert_eq!(event["body"]["snapshotIds"][0], snapshot_id.as_str());
    Ok(())
}

#[test]
fn successful_save_registers_file_row_volumes_baseline_and_committed_event()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, version) = result_store(70, false, true, true)?;
    let files = MemoryFiles::default();
    let index = MemoryIndex::default();
    let rig = rig(store, index.clone(), files.clone(), true)?;
    let response = rig
        .command
        .execute(request(version.clone(), true), &caller()?)
        .map_err(reason)?;
    assert!(serde_json::to_value(&response)?.get("pinApplied").is_none());
    let job_id = match response {
        SnapshotSaveResponse::Accepted(accepted) => accepted.job_id,
        _ => return Err("expected accepted response".into()),
    };
    assert_eq!(rig.runtime.run_next()?, Some(job_id));
    let state = index.0.lock().map_err(|_| "index poisoned")?;
    assert_eq!(state.list_seq, 1);
    let saved_snapshot_id = verify_saved_row(&state, &version, &files)?;
    verify_saved_volume(&state);
    drop(state);
    assert_eq!(files.state().temporary_count, 0);
    verify_saved_event(&rig.events, &saved_snapshot_id)?;
    let before_duplicate = files.state();
    let repeated = rig
        .command
        .execute(request(version, false), &caller()?)
        .map_err(reason)?;
    assert_eq!(repeated, existing_snapshot(saved_snapshot_id));
    let after_duplicate = files.state();
    assert_eq!(
        after_duplicate.capacity_queries,
        before_duplicate.capacity_queries
    );
    assert_eq!(after_duplicate.create_calls, before_duplicate.create_calls);
    Ok(())
}

#[test]
fn write_and_registration_failures_leave_no_index_row_or_event()
-> Result<(), Box<dyn std::error::Error>> {
    for (number, fail_write, not_durable, fail_index, keep_file) in [
        (80, true, false, false, false),
        (90, false, true, false, true),
        (100, false, false, true, true),
    ] {
        let (store, version) = result_store(number, false, true, true)?;
        let files = MemoryFiles::default();
        files.configure(|state| {
            state.available = Some(u64::MAX);
            state.fail_write = fail_write;
            state.registered_not_durable = not_durable;
        });
        let index = MemoryIndex::default();
        index.0.lock().map_err(|_| "index poisoned")?.fail_register = fail_index;
        let rig = rig(store, index.clone(), files.clone(), true)?;
        let response = rig
            .command
            .execute(request(version, false), &caller()?)
            .map_err(reason)?;
        let job_id = match response {
            SnapshotSaveResponse::Accepted(accepted) => accepted.job_id,
            _ => return Err("expected accepted response".into()),
        };
        assert_eq!(rig.runtime.run_next()?, Some(job_id));
        assert!(
            index
                .0
                .lock()
                .map_err(|_| "index poisoned")?
                .rows
                .is_empty()
        );
        assert_eq!(files.state().temporary_count, 0);
        assert_eq!(!files.state().finals.is_empty(), keep_file);
        std::thread::sleep(Duration::from_millis(110));
        assert!(rig.events.next_json().is_none());
    }
    Ok(())
}

struct CancelContext;

impl JobContext for CancelContext {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Cancel
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Err(JobFlowError::UnsupportedResource)
    }
}

#[test]
fn cancellation_at_first_record_drops_the_pending_file() -> Result<(), Box<dyn std::error::Error>> {
    let (store, version_id) = result_store(120, false, true, true)?;
    let version = store
        .pin_current(&version_id)
        .map_err(|error| format!("{error:?}"))?;
    let files = MemoryFiles::default();
    let index = MemoryIndex::default();
    let rig = rig(store, index.clone(), files.clone(), true)?;
    let job = SaveJob {
        context: rig.command.context.clone(),
        version,
        source_version_id: version_id,
        lineage_id: id(121)?,
        snapshot_id: id(122)?,
        pin: false,
    };
    let result = job.save(Arc::new(CancelContext));
    assert!(matches!(result, Err(SaveFailure::Cancelled)));
    assert_eq!(files.state().temporary_count, 0);
    assert!(files.state().finals.is_empty());
    assert!(
        index
            .0
            .lock()
            .map_err(|_| "index poisoned")?
            .rows
            .is_empty()
    );
    Ok(())
}
