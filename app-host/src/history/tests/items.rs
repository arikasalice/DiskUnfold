use super::*;
use du_core_contracts::history_ipc::{HistoryItemOutcome, HistoryItemsResponse};
use rusqlite::{Connection, params};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn item_ipc_pins_counts_and_rows_and_distinguishes_unreadable_results() -> TestResult {
    let dir = tempfile::tempdir()?;
    let database = crate::startup_database::open(dir.path())?;
    append(&database, 1, 100)?;
    let sql = Connection::open(dir.path().join("diskunfold.db"))?;
    let engine = engine(dir.path(), database)?;
    let job = Id::from_v7_bytes([1; 16]);

    let summary_only = page(&engine, json!({"jobRef": job}))?;
    assert_eq!(summary_only.counts.summary, None);
    assert_eq!(summary_only.counts.recorded, "0");
    assert_eq!(
        summary_only.completeness,
        HistoryDetailCompleteness::Complete
    );

    seed_items(&sql)?;
    let first = page(&engine, json!({"jobRef": job, "count": 1}))?;
    let cursor = first.next_cursor.ok_or("missing cursor")?;
    assert_eq!(first.counts.summary.as_deref(), Some("5"));
    assert_eq!(first.counts.recorded, "4");
    assert_eq!(first.counts.unavailable.as_deref(), Some("1"));
    assert_eq!(first.rows[0].item_seq, "0");

    insert(&sql, 4, "completed", false)?;
    let next = page(&engine, json!({"jobRef": job, "cursor": cursor}))?;
    assert_eq!(next.counts, first.counts);
    assert_eq!(
        next.rows
            .iter()
            .map(|row| row.item_seq.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    assert_eq!(next.missing_reasons, ["read_failed", "items_incomplete"]);
    assert!(next.rows[2].result.is_none());
    assert!(next.next_cursor.is_none());
    assert_displaced(&engine, &job)?;
    assert_invalid_cursors(&engine, &job, &cursor)?;

    sql.execute(
        "UPDATE job_runtime_history_state SET delete_generation=1",
        [],
    )?;
    let stale = ipc(
        &engine,
        "history.items",
        json!({"jobRef": job, "cursor": cursor}),
    )?
    .error
    .ok_or("no stale error")?;
    assert_eq!(stale.code, ErrorCode::StaleVersion);
    assert_eq!(stale.reason_key, "history.version_changed");
    Ok(())
}

fn seed_items(sql: &Connection) -> TestResult {
    sql.execute_batch(
        "DROP TRIGGER job_runtime_job_record_no_update;
         UPDATE job_runtime_job_record SET completed_count=5;",
    )?;
    insert(sql, 0, "completed", false)?;
    insert(sql, 1, "overwritten", true)?;
    insert(sql, 2, "failed", true)?;
    insert(sql, 3, "failed", false)?;
    sql.execute_batch(
        "DROP TRIGGER job_runtime_item_result_no_update;
         PRAGMA ignore_check_constraints=ON;
         UPDATE job_runtime_item_result SET observed_size_state='unrecognized' WHERE item_seq=3;",
    )?;
    Ok(())
}

fn insert(sql: &Connection, seq: i64, outcome: &str, displaced: bool) -> TestResult {
    sql.execute(
        "INSERT INTO job_runtime_item_result (
             job_id, item_seq, item_ref, location_raw, location_text, outcome,
             review_needed, held, outcome_unknown, observed_size_state, displaced_to_trash,
             displaced_trash_ref, displaced_name, displaced_original_location,
             displaced_trashed_at_ms, created_at_ms)
         VALUES (X'01010101010171018101010101010101', ?1, X'ABCD', X'CAFE',
                 'recorded/file', ?2, 0, 0, 0, 'unknown', ?3, ?4, ?5, ?6, ?7, 100)",
        params![
            seq,
            outcome,
            displaced,
            displaced.then_some(b"private handle".as_slice()),
            displaced.then_some("previous.txt"),
            displaced.then_some("recorded/previous.txt"),
            displaced.then_some(90)
        ],
    )?;
    Ok(())
}

fn engine(
    dir: &std::path::Path,
    database: SharedDatabase,
) -> Result<Engine, Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    du_engine_api::history::items::register(
        &mut builder,
        HistoryReader::open(dir, database)?,
        &du_platform_os::entropy::OsRandom,
    )
    .map_err(|error| format!("{error:?}"))?;
    Ok(builder.build()?)
}

fn page(engine: &Engine, body: Value) -> Result<HistoryItemsResponse, Box<dyn std::error::Error>> {
    let envelope = ipc(engine, "history.items", body)?;
    assert!(envelope.error.is_none(), "{:?}", envelope.error);
    Ok(serde_json::from_value(envelope.body)?)
}

fn assert_displaced(engine: &Engine, job: &Id) -> TestResult {
    let response = ipc(
        engine,
        "history.items",
        json!({"jobRef": job, "outcome": "overwritten"}),
    )?;
    assert!(response.error.is_none());
    let wire = response.body.to_string();
    for field in [
        "locationRaw",
        "itemRef",
        "displacedTrashRef",
        "private handle",
    ] {
        assert!(!wire.contains(field), "unexpected private field {field}");
    }
    let filtered: HistoryItemsResponse = serde_json::from_value(response.body)?;
    let item = filtered.rows[0]
        .result
        .as_ref()
        .ok_or("missing overwritten result")?;
    assert_eq!(item.outcome, HistoryItemOutcome::Overwritten);
    assert_eq!(filtered.counts.recorded, "5");
    let displaced = item.displaced.as_ref().ok_or("missing displaced item")?;
    assert!(displaced.ambiguous);
    assert_eq!(displaced.name.as_deref(), Some("previous.txt"));
    Ok(())
}

fn assert_invalid_cursors(engine: &Engine, job: &Id, cursor: &str) -> TestResult {
    let other = Id::from_v7_bytes([2; 16]);
    for request in [
        json!({"jobRef": other, "cursor": cursor}),
        json!({"jobRef": job, "cursor": cursor, "outcome": "failed"}),
        json!({"jobRef": job, "cursor": format!("{cursor}x")}),
    ] {
        assert_eq!(
            ipc(engine, "history.items", request)?
                .error
                .ok_or("cursor accepted")?
                .reason_key,
            "cursor.invalid"
        );
    }
    let removed = ipc(engine, "history.items", json!({"jobRef": other}))?
        .error
        .ok_or("missing row accepted")?;
    assert_eq!(removed.code, ErrorCode::NotFound);
    assert_eq!(removed.reason_key, "history.removed");
    Ok(())
}
