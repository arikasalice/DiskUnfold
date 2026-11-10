use super::*;
use rusqlite::{Connection, params};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type SchemaObject = (String, String, String);

fn schema_objects(connection: &Connection) -> rusqlite::Result<Vec<SchemaObject>> {
    let mut statement = connection.prepare(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE name NOT GLOB 'sqlite_*' ORDER BY type, name",
    )?;
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect()
}

#[test]
fn fresh_and_migrated_databases_use_the_same_verified_definitions() -> TestResult {
    let dir = tempfile::tempdir()?;
    let fresh_path = dir.path().join("fresh.db");
    let legacy_path = dir.path().join("legacy.db");
    let fresh =
        AppDb::create_gui_with_history(&fresh_path).map_err(|error| format!("{error:?}"))?;
    drop(AppDb::create_gui(&legacy_path).map_err(|error| format!("{error:?}"))?);
    assert!(matches!(
        AppDb::open_gui_with_history(&legacy_path),
        Err(OpenError::HistorySchema(error)) if matches!(*error, MigrationError::VersionMismatch)
    ));
    for definition in HISTORY_DEFINITIONS.steps {
        let backup = dir.path().join(format!("before-{}.db", definition.number));
        assert_eq!(
            migration::apply_next(&legacy_path, &backup, &HISTORY_DEFINITIONS)
                .map_err(|error| format!("{error:?}"))?,
            definition.number
        );
    }
    let migrated =
        AppDb::open_gui_with_history(&legacy_path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(fresh.schema_version()?, HISTORY_SCHEMA_VERSION);
    assert_eq!(migrated.schema_version()?, HISTORY_SCHEMA_VERSION);
    assert_eq!(
        schema_objects(&fresh.connection)?,
        schema_objects(&migrated.connection)?
    );
    Ok(())
}

#[test]
fn backfills_links_and_advances_sequence_from_existing_summaries() -> TestResult {
    use du_core_contracts::{
        Id,
        job::{HistoryAppend, JobState, JobSummary},
    };

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("older-history.db");
    drop(AppDb::create_gui(&path).map_err(|error| format!("{error:?}"))?);
    migration::apply_next(
        &path,
        &dir.path().join("before-summary.db"),
        &HISTORY_DEFINITIONS,
    )
    .map_err(|error| format!("{error:?}"))?;
    let reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(reader.schema_version()?, 2);
    assert!(
        reader
            .connection
            .execute("UPDATE app_db_purge_gate SET open=1", [])
            .is_err()
    );
    drop(reader);
    assert!(matches!(
        AppDb::open_gui_with_history(&path),
        Err(OpenError::HistorySchema(error)) if matches!(*error, MigrationError::VersionMismatch)
    ));
    let older = Connection::open(&path)?;
    for (item, finished) in [(3, 200), (2, 100), (1, 100)] {
        let mut id = [0u8; 16];
        id[6] = 0x70;
        id[8] = 0x80;
        id[15] = item;
        older.execute(
            "INSERT INTO job_runtime_job_record
                (job_id, kind_id, kind_version, scope_summary, final_state, completed_count,
                 overwritten_count, failed_count, skipped_count, unprocessed_count,
                 finished_at_ms, correlation_id, created_at_ms)
             VALUES (?1, 'scan.metadata', 1, 'metadata: 1 root', 'done', 0, 0, 0, 0, 0, ?2, ?1, 300)",
            params![id.as_slice(), finished],
        )?;
    }
    drop(older);
    migration::apply_next(
        &path,
        &dir.path().join("before-link.db"),
        &HISTORY_DEFINITIONS,
    )
    .map_err(|error| format!("{error:?}"))?;
    migration::apply_next(
        &path,
        &dir.path().join("before-recent-scope.db"),
        &HISTORY_DEFINITIONS,
    )
    .map_err(|error| format!("{error:?}"))?;
    let mut db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let mut statement = db.connection.prepare(
        "SELECT link.job_id, link.record_seq, link.reckon_at_ms, link.record_origin
         FROM job_runtime_job_link link JOIN job_runtime_job_record record USING (job_id)
         ORDER BY link.record_seq",
    )?;
    let links = statement
        .query_map([], |row| {
            let id: Vec<u8> = row.get(0)?;
            Ok((
                id[15],
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert_eq!(
        links,
        [
            (1, 1, 100, "this_app".into()),
            (2, 2, 100, "this_app".into()),
            (3, 3, 200, "this_app".into()),
        ]
    );
    let state: (i64, i64) = db.connection.query_row(
        "SELECT delete_generation, last_record_seq FROM job_runtime_history_state",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(state, (0, 3));
    drop(statement);
    let next = Id::from_v7_bytes([4; 16]);
    let job = JobSummary {
        job_id: next.clone(),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: Default::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(310),
        finished_at_ms: 320,
        correlation_id: next,
    };
    assert_eq!(db.append_scan_history(&job, 325), Ok(HistoryAppend::Added));
    let sequence: i64 = db.connection.query_row(
        "SELECT last_record_seq FROM job_runtime_history_state",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(sequence, 4);
    Ok(())
}

#[test]
fn history_item_constraints_allow_failed_moves_with_verified_displacement() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = AppDb::create_gui_with_history(&dir.path().join("history.db"))
        .map_err(|error| format!("{error:?}"))?;
    let sql = "INSERT INTO job_runtime_item_result
        (job_id, item_seq, item_ref, location_raw, location_text, outcome,
         review_needed, held, outcome_unknown, observed_size, observed_size_state,
         displaced_to_trash, displaced_name, created_at_ms)
        VALUES (zeroblob(16), ?1, X'01', X'64', 'd', ?2, 0, 0, 0, ?3, ?4, ?5, ?6, 10)";
    assert!(
        db.connection
            .execute(
                sql,
                params![0, "failed", None::<i64>, "unknown", 0, "original"]
            )
            .is_err()
    );
    assert!(
        db.connection
            .execute(sql, params![0, "completed", 0, "exact", 1, "original"])
            .is_err()
    );
    assert!(
        db.connection
            .execute(
                sql,
                params![0, "overwritten", 0, "exact", 0, None::<String>]
            )
            .is_err()
    );
    assert!(
        db.connection
            .execute(sql, params![0, "failed", 0, "unknown", 1, "original"])
            .is_err()
    );
    db.connection.execute(
        sql,
        params![0, "failed", None::<i64>, "unknown", 1, "original"],
    )?;
    db.connection.execute(
        sql,
        params![1, "overwritten", 0, "exact", 1, None::<String>],
    )?;
    let replace = sql.replacen("INSERT INTO", "INSERT OR REPLACE INTO", 1);
    assert!(
        db.connection
            .execute(
                &replace,
                params![0, "failed", None::<i64>, "unknown", 1, "replacement"],
            )
            .is_err()
    );
    assert!(
        db.connection
            .execute("DELETE FROM job_runtime_item_result", [])
            .is_err()
    );
    assert!(
        db.connection
            .execute("UPDATE job_runtime_item_result SET held=1", [])
            .is_err()
    );
    Ok(())
}

#[test]
fn history_database_with_modified_definition_digest_is_rejected_on_open() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("tampered.db");
    let db = AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    db.connection.execute_batch(
        "DROP TRIGGER app_db_schema_meta_no_update;
         UPDATE app_db_schema_meta SET definition_hash=zeroblob(32) WHERE migration_no=2;",
    )?;
    drop(db);
    for result in [
        AppDb::open_gui_with_history(&path),
        AppDb::open_read_only(&path),
    ] {
        assert!(matches!(
            result,
            Err(OpenError::HistorySchema(error)) if matches!(*error, MigrationError::Tampered)
        ));
    }
    Ok(())
}
