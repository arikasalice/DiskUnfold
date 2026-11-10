use super::*;
use du_core_contracts::job::JobCounts;
use std::{
    path::Path,
    sync::{Arc, Barrier},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn id(n: u8) -> Id {
    let mut bytes = [0; 16];
    bytes[15] = n;
    Id::from_v7_bytes(bytes)
}

fn summary(n: u8) -> JobSummary {
    JobSummary {
        job_id: id(n),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: JobCounts {
            completed: 10,
            ..JobCounts::default()
        },
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: Some(id(90)),
        started_at_ms: Some(100),
        finished_at_ms: 200,
        correlation_id: id(91),
    }
}

fn installed(path: &Path) -> Result<AppDb, Box<dyn std::error::Error>> {
    AppDb::create_gui_with_history(path).map_err(|error| format!("{error:?}").into())
}

fn counts(db: &AppDb) -> rusqlite::Result<(i64, i64, i64)> {
    db.connection.query_row(
        "SELECT (SELECT count(*) FROM job_runtime_job_record),
                (SELECT count(*) FROM job_runtime_job_link),
                last_record_seq FROM job_runtime_history_state",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
}

#[test]
fn persists_summary_link_and_sequence_without_rewriting_on_retry() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let mut db = installed(&path)?;
    let mut job = summary(1);
    assert_eq!(db.append_scan_history(&job, 250), Ok(HistoryAppend::Added));
    job.counts.completed = 999;
    assert_eq!(
        db.append_scan_history(&job, 999),
        Ok(HistoryAppend::Duplicate)
    );
    drop(db);

    let db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(counts(&db)?, (1, 1, 1));
    let summary_row = db.connection.query_row(
        "SELECT completed_count, scope_summary, finished_at_ms, created_at_ms
         FROM job_runtime_job_record",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get::<_, String>(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        },
    )?;
    assert_eq!(
        summary_row,
        (10i64, "metadata: 1 root".into(), 200i64, 250i64)
    );
    let link = db.connection.query_row(
        "SELECT record_origin, reckon_at_ms, record_seq, created_at_ms,
                plan_version_id IS NULL AND plan_lineage_id IS NULL AND plan_hash IS NULL
                AND predecessor_job_id IS NULL AND relation_kind IS NULL
         FROM job_runtime_job_link WHERE job_id = (SELECT job_id FROM job_runtime_job_record)",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    assert_eq!(link, ("this_app".into(), 200i64, 1i64, 250i64, true));
    for sql in [
        "UPDATE job_runtime_job_record SET completed_count=4",
        "UPDATE job_runtime_job_link SET record_seq=2",
        "DELETE FROM job_runtime_job_record",
        "DELETE FROM job_runtime_job_link",
    ] {
        assert!(db.connection.execute(sql, []).is_err(), "{sql}");
    }
    Ok(())
}

#[test]
fn rejects_raw_fields_nonterminal_and_unregistered_scan_versions() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir.path().join("untrusted.db"))?;
    for field in 0..7 {
        let mut job = summary(field + 1);
        match field {
            0 => job.scope_summary = r"C:\private\file.txt".into(),
            1 => job.failure_reason_key = Some("/home/user/private".into()),
            2 => job.unprocessed_summary = Some("/home/user/private".into()),
            3 => job.kind_id = "C:/private".into(),
            4 => job.counts.completed = u64::MAX,
            5 => job.state = JobState::Running,
            _ => job.kind_version = 2,
        }
        assert_eq!(
            db.append_scan_history(&job, 250),
            Err("history.record_failed".into())
        );
    }
    assert_eq!(counts(&db)?, (0, 0, 0));
    Ok(())
}

#[test]
fn rolls_back_all_history_on_link_failure_and_leaves_busy_to_caller() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("rollback.db");
    let mut db = installed(&path)?;
    db.connection.busy_timeout(std::time::Duration::ZERO)?;
    db.connection.execute_batch(
        "CREATE TRIGGER fail_history AFTER INSERT ON job_runtime_job_link
         BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    )?;
    assert_eq!(
        db.append_scan_history(&summary(1), 250),
        Err("history.record_failed".into())
    );
    assert_eq!(counts(&db)?, (0, 0, 0));
    db.connection.execute_batch("DROP TRIGGER fail_history")?;

    let other = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    other.connection.execute_batch("BEGIN IMMEDIATE")?;
    assert_eq!(
        db.append_scan_history(&summary(1), 250),
        Err("db.busy".into())
    );
    other.connection.execute_batch("ROLLBACK")?;
    assert_eq!(
        db.append_scan_history(&summary(1), 250),
        Ok(HistoryAppend::Added)
    );
    assert_eq!(counts(&db)?, (1, 1, 1));
    Ok(())
}

#[test]
fn refuses_partial_history_and_sequence_exhaustion() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir.path().join("inconsistent.db"))?;
    assert_eq!(
        db.append_scan_history(&summary(1), 250),
        Ok(HistoryAppend::Added)
    );
    db.connection.execute_batch(
        "BEGIN IMMEDIATE;
         UPDATE app_db_purge_gate SET open=1;
         DELETE FROM job_runtime_job_link;
         UPDATE app_db_purge_gate SET open=0;
         COMMIT;
         UPDATE job_runtime_history_state SET last_record_seq=9223372036854775807;",
    )?;
    for n in [1, 2] {
        assert_eq!(
            db.append_scan_history(&summary(n), 251),
            Err("history.record_failed".into())
        );
    }
    assert_eq!(counts(&db)?, (1, 0, i64::MAX));
    Ok(())
}

#[test]
fn concurrent_connections_allocate_one_sequence_per_job() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("concurrent.db");
    drop(installed(&path)?);
    let gate = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for n in [1, 2, 1] {
        let mut db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
        let gate = gate.clone();
        workers.push(std::thread::spawn(move || {
            gate.wait();
            db.append_scan_history(&summary(n), 250)
        }));
    }
    let mut duplicates = 0;
    for worker in workers {
        match worker.join().map_err(|_| "writer panicked")?? {
            HistoryAppend::Added => {}
            HistoryAppend::Duplicate => duplicates += 1,
        }
    }
    assert_eq!(duplicates, 1);
    let db = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(counts(&db)?, (2, 2, 2));
    Ok(())
}

#[test]
fn append_leaves_legacy_settings_database_untouched() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("legacy.db");
    let mut db = AppDb::create_gui(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        db.append_scan_history(&summary(1), 250),
        Err("history.record_failed".into())
    );
    assert_eq!(db.schema_version()?, 1);
    assert!(du_core_contracts::settings::SettingsStore::load(&mut db).is_ok());
    let history_objects: i64 = db.connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'job_runtime_%'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(history_objects, 0);
    Ok(())
}

#[test]
fn legacy_settings_survive_both_history_definitions_and_scan_append() -> TestResult {
    use crate::{HISTORY_DEFINITIONS, migration::apply_next};
    use rusqlite::Connection;

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("legacy.db");
    let db = AppDb::create_gui(&path).map_err(|error| format!("{error:?}"))?;
    db.connection.execute_batch(
        "INSERT INTO user_env_setting VALUES
             ('locale.display_language', '\"ko\"', 'screen', 1, 2, 1);
         UPDATE user_env_setting_version SET settings_version=7;",
    )?;
    drop(db);

    let first_backup = dir.path().join("before-summary.db");
    let second_backup = dir.path().join("before-link.db");
    assert_eq!(
        apply_next(&path, &first_backup, &HISTORY_DEFINITIONS)
            .map_err(|error| format!("{error:?}"))?,
        2
    );
    let mut db = AppDb {
        connection: Connection::open(&path)?,
    };
    assert_eq!(
        db.append_scan_history(&summary(1), 250),
        Err("history.record_failed".into())
    );
    let count: i64 =
        db.connection
            .query_row("SELECT count(*) FROM job_runtime_job_record", [], |row| {
                row.get(0)
            })?;
    assert_eq!(count, 0);
    drop(db);

    assert_eq!(
        apply_next(&path, &second_backup, &HISTORY_DEFINITIONS)
            .map_err(|error| format!("{error:?}"))?,
        3
    );
    let mut db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        db.append_scan_history(&summary(1), 250),
        Ok(HistoryAppend::Added)
    );
    assert_eq!(counts(&db)?, (1, 1, 1));
    for connection in [&db.connection, &Connection::open(&first_backup)?] {
        let settings: (String, i64) = connection.query_row(
            "SELECT
                (SELECT value_json FROM user_env_setting WHERE setting_key = 'locale.display_language'),
                settings_version FROM user_env_setting_version",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(settings, ("\"ko\"".into(), 7));
    }
    Ok(())
}
