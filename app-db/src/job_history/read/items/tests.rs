use super::*;
use du_core_contracts::{
    Id,
    job::{JobCounts, JobState, JobSummary},
};

type Fixture = (tempfile::TempDir, AppDb, AppDb, Id);

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("items.db");
    let mut writer = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let id = Id::from_v7_bytes([1; 16]);
    let summary = JobSummary {
        job_id: id.clone(),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: JobCounts::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(1),
        finished_at_ms: 2,
        correlation_id: id.clone(),
    };
    writer.append_scan_history(&summary, 3)?;
    let reader = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
    Ok((dir, writer, reader, id))
}

fn insert(db: &AppDb, id: &Id, n: i64, outcome: &str) -> rusqlite::Result<()> {
    let key = super::super::super::id_bytes(id).ok_or(rusqlite::Error::InvalidQuery)?;
    db.connection.execute("INSERT INTO job_runtime_item_result (job_id, item_seq, item_ref, location_raw, location_text, outcome, review_needed, held, outcome_unknown, observed_size, observed_size_state, displaced_to_trash, displaced_trash_ref, displaced_name, displaced_original_location, displaced_trashed_at_ms, created_at_ms) VALUES (?1, ?2, X'534543524554', X'524157534543524554', 'display/path', ?3, 0, 0, 0, 12, 'exact', ?4, ?5, ?6, ?7, ?8, 1)", params![key.as_slice(), n, outcome, i64::from(outcome == "overwritten"), if outcome == "overwritten" { Some(b"TRASH_SECRET".as_slice()) } else { None }, if outcome == "overwritten" { Some("name") } else { None }, if outcome == "overwritten" { Some("old/display") } else { None }, if outcome == "overwritten" { Some(10) } else { None }])?;
    Ok(())
}

#[test]
fn pages_filter_and_omit_sensitive_raw_values() -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, writer, mut reader, id) = fixture()?;
    insert(&writer, &id, 0, "completed")?;
    insert(&writer, &id, 1, "overwritten")?;
    insert(&writer, &id, 2, "failed")?;
    let first = reader
        .read_history_items(&id, None, None, 1)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.rows[0].item_seq, 0);
    let second = reader
        .read_history_items(&id, None, first.next.clone(), 1)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(second.rows[0].item_seq, 1);
    let item = second.rows[0].item.as_ref().ok_or("missing")?;
    assert_eq!(item.location_display, "display/path");
    assert_eq!(
        item.displaced
            .as_ref()
            .and_then(|v| v.original_location_display.as_deref()),
        Some("old/display")
    );
    let rendered = format!("{second:?}");
    assert!(
        !rendered.contains("RAWSECRET")
            && !rendered.contains("TRASH_SECRET")
            && !rendered.contains("item_ref")
    );
    let filtered = reader
        .read_history_items(&id, Some(Outcome::Overwritten), None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(filtered.rows.len(), 1);
    assert_eq!(filtered.rows[0].item_seq, 1);
    assert!(matches!(
        reader.read_history_items(&id, Some(Outcome::Failed), first.next, 10),
        Err(Error::InvalidRequest)
    ));
    Ok(())
}

#[test]
fn invalid_row_missing_job_and_changed_generation_are_distinct()
-> Result<(), Box<dyn std::error::Error>> {
    let (_dir, writer, mut reader, id) = fixture()?;
    insert(&writer, &id, 0, "completed")?;
    insert(&writer, &id, 1, "completed")?;
    writer.connection.execute_batch("DROP TRIGGER job_runtime_item_result_no_update; PRAGMA ignore_check_constraints=ON; UPDATE job_runtime_item_result SET outcome='unknown' WHERE item_seq=0;")?;
    let page = reader
        .read_history_items(&id, None, None, 1)
        .map_err(|e| format!("{e:?}"))?;
    assert!(page.rows[0].item.is_none());
    assert!(matches!(
        reader.read_history_items(&Id::from_v7_bytes([9; 16]), None, None, 1),
        Err(Error::NotFound)
    ));
    let mut bad = page.next.clone().ok_or("missing cursor")?;
    bad.job_id = Id::from_v7_bytes([9; 16]);
    assert!(matches!(
        reader.read_history_items(&id, None, Some(bad), 1),
        Err(Error::InvalidRequest)
    ));
    writer.connection.execute(
        "UPDATE job_runtime_history_state SET delete_generation=delete_generation+1",
        [],
    )?;
    assert!(matches!(
        reader.read_history_items(&id, None, page.next, 1),
        Err(Error::VersionChanged)
    ));
    writer.connection.execute_batch(
        "UPDATE app_db_purge_gate SET open=1;
        DELETE FROM job_runtime_job_link;
        UPDATE app_db_purge_gate SET open=0;",
    )?;
    assert!(matches!(
        reader.read_history_items(&id, None, None, 1),
        Err(Error::NotFound)
    ));
    Ok(())
}

#[test]
fn snapshot_and_cursor_fence_concurrent_appends_and_deletion()
-> Result<(), Box<dyn std::error::Error>> {
    let (_dir, writer, mut reader, id) = fixture()?;
    insert(&writer, &id, 0, "completed")?;
    insert(&writer, &id, 1, "completed")?;
    let first = reader
        .read_history_items_with(&id, None, None, 1, || {
            insert(&writer, &id, 2, "completed").ok();
            writer
                .connection
                .execute(
                    "UPDATE job_runtime_history_state SET delete_generation=1",
                    [],
                )
                .ok();
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.revision.delete_generation, 0);
    // The read transaction was anchored before both writes, including the bound.
    assert_eq!(first.next.as_ref().ok_or("missing cursor")?.max_item_seq, 1);
    assert!(matches!(
        reader.read_history_items(&id, None, first.next, 1),
        Err(Error::VersionChanged)
    ));
    let current = reader
        .read_history_items(&id, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(current.rows.len(), 3);
    Ok(())
}
