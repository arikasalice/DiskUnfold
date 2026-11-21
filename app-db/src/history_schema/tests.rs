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
    migration::apply_next(
        &path,
        &dir.path().join("before-ruleset.db"),
        &HISTORY_DEFINITIONS,
    )
    .map_err(|error| format!("{error:?}"))?;
    migration::apply_next(
        &path,
        &dir.path().join("before-group-rules.db"),
        &HISTORY_DEFINITIONS,
    )
    .map_err(|error| format!("{error:?}"))?;
    migration::apply_next(
        &path,
        &dir.path().join("before-scope-observation.db"),
        &HISTORY_DEFINITIONS,
    )
    .map_err(|error| format!("{error:?}"))?;
    migration::apply_next(
        &path,
        &dir.path().join("before-scope-roots.db"),
        &HISTORY_DEFINITIONS,
    )
    .map_err(|error| format!("{error:?}"))?;
    for definition in HISTORY_DEFINITIONS
        .steps
        .iter()
        .filter(|step| step.number > 8)
    {
        migration::apply_next(
            &path,
            &dir.path().join(format!("before-{}.db", definition.number)),
            &HISTORY_DEFINITIONS,
        )
        .map_err(|error| format!("{error:?}"))?;
    }
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
    let mut job = JobSummary {
        job_id: next.clone(),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        scope_roots: Vec::new(),
        state: JobState::Done,
        counts: Default::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(310),
        finished_at_ms: 320,
        correlation_id: next,
    };
    job.scope_roots.push("/fixture/migrated".into());
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
    assert_eq!(
        AppDb::probe_schema_version_read_only(&path).map_err(|error| format!("{error:?}"))?,
        HISTORY_SCHEMA_VERSION
    );
    for result in [
        AppDb::open_gui_with_history(&path),
        AppDb::open_gui_with_history_after_version_probe(&path),
        AppDb::open_read_only(&path),
    ] {
        assert!(matches!(
            result,
            Err(OpenError::HistorySchema(error)) if matches!(*error, MigrationError::Tampered)
        ));
    }
    Ok(())
}

#[test]
fn current_version_probe_does_not_migrate_a_database_changed_before_writer_open() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("changed-probe.db");
    drop(AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?);
    let predecessor = HISTORY_DEFINITIONS
        .steps
        .iter()
        .rev()
        .nth(1)
        .map(|definition| definition.number)
        .ok_or("missing predecessor schema definition")?;

    assert_eq!(
        AppDb::probe_schema_version_read_only(&path).map_err(|error| format!("{error:?}"))?,
        HISTORY_SCHEMA_VERSION
    );
    let changed = Connection::open(&path)?;
    changed.execute_batch("DROP TRIGGER app_db_schema_meta_no_delete;")?;
    changed.execute(
        "DELETE FROM app_db_schema_meta WHERE migration_no=?1",
        [HISTORY_SCHEMA_VERSION],
    )?;
    changed.pragma_update(None, "user_version", predecessor)?;
    drop(changed);

    let error = AppDb::open_gui_with_history_after_version_probe(&path)
        .err()
        .ok_or("writer accepted the changed database version")?;
    assert!(matches!(
        error,
        OpenError::HistorySchema(error) if matches!(*error, MigrationError::VersionMismatch)
    ));
    assert!(!dir.path().join("db-backups").exists());
    let reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(reader.schema_version()?, predecessor);
    Ok(())
}

#[test]
fn writer_open_error_is_preserved_when_read_only_fallback_passes() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("writer-open-failure.db");
    drop(AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?);

    let error = super::open_after_version_probe(&path, |_, _| Err(OpenError::WalUnavailable))
        .err()
        .ok_or("writer error was not returned")?;
    assert!(matches!(error, OpenError::WalUnavailable));
    Ok(())
}

#[test]
fn latest_migrations_preserve_predecessor_schema_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let predecessor_version = HISTORY_SCHEMA_VERSION - 2;
    let path = dir.path().join(format!("schema-{predecessor_version}.db"));
    drop(AppDb::create_gui(&path).map_err(|error| format!("{error:?}"))?);

    for definition in HISTORY_DEFINITIONS.steps {
        if definition.number > predecessor_version {
            break;
        }
        let backup = dir.path().join(format!("before-{}.db", definition.number));
        assert_eq!(
            migration::apply_next(&path, &backup, &HISTORY_DEFINITIONS)
                .map_err(|error| format!("{error:?}"))?,
            definition.number
        );
    }

    let legacy = Connection::open(&path)?;
    legacy.execute(
        "INSERT INTO change_planner_ruleset
         (version,ruleset_hash,hash_format,class_table_version,
          known_location_table_version,rule_format_version,origin,
          change_summary,body,created_at_ms)
         VALUES (1,?1,'du.ruleset.hash.v1','class.1','location.1',
                 1,'gui','{}','{}',100)",
        [vec![1; 32]],
    )?;
    legacy.execute(
        "INSERT INTO job_runtime_job_record
         (job_id,kind_id,kind_version,scope_summary,final_state,completed_count,
          overwritten_count,failed_count,skipped_count,unprocessed_count,
          finished_at_ms,correlation_id,created_at_ms)
         VALUES (?1,'scan.metadata',1,'metadata: 1 root','done',1,0,0,0,0,200,?2,210)",
        params![vec![2u8; 16], vec![3u8; 16]],
    )?;
    let old_version: u32 = legacy.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(old_version, predecessor_version);
    drop(legacy);

    assert_eq!(
        migration::apply_next(
            &path,
            &dir.path()
                .join(format!("before-{}.db", predecessor_version + 1)),
            &HISTORY_DEFINITIONS,
        )
        .map_err(|error| format!("{error:?}"))?,
        predecessor_version + 1
    );
    assert_eq!(
        migration::apply_next(
            &path,
            &dir.path()
                .join(format!("before-{HISTORY_SCHEMA_VERSION}.db")),
            &HISTORY_DEFINITIONS,
        )
        .map_err(|error| format!("{error:?}"))?,
        HISTORY_SCHEMA_VERSION
    );

    let migrated = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let ruleset: Vec<u8> = migrated.connection.query_row(
        "SELECT ruleset_hash FROM change_planner_ruleset WHERE version = 1",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(ruleset, vec![1; 32]);
    let job_count: i64 = migrated.connection.query_row(
        "SELECT count(*) FROM job_runtime_job_record
         WHERE final_state = 'done' AND completed_count = 1",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(job_count, 1);
    assert_eq!(migrated.schema_version()?, HISTORY_SCHEMA_VERSION);
    let plan_count: i64 = migrated.connection.query_row(
        "SELECT count(*) FROM change_planner_plan_version",
        [],
        |row| row.get(0),
    )?;
    let check_count: i64 = migrated.connection.query_row(
        "SELECT count(*) FROM job_runtime_approval_check",
        [],
        |row| row.get(0),
    )?;
    assert_eq!((plan_count, check_count), (0, 0));
    Ok(())
}
