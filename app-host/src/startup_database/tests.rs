use super::*;
use du_core_contracts::{
    Id,
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
};
use du_engine_api::desktop::DesktopEvents;
use rusqlite::{Connection, OpenFlags};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn startup_migrates_preserves_settings_and_keeps_lock_until_last_writer() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    drop(AppDb::create_gui(&path).map_err(open_error)?);
    let legacy = Connection::open(&path)?;
    legacy.execute_batch(
        "INSERT INTO user_env_setting VALUES
             ('space.shared_table_max_entries', '200000', 'screen', 1, 2, 1);
         UPDATE user_env_setting_version SET settings_version=7;",
    )?;
    drop(legacy);

    let database = open(dir.path())?;
    let settings = crate::settings::StartupSettings::load(&database, &["en", "ko"])?;
    assert_eq!(settings.scan_limit_source()().entries(), 200_000);
    let history = database.history(|| 250);
    drop(database);
    assert!(matches!(open(dir.path()), Err(error) if error.kind() == io::ErrorKind::AlreadyExists));
    let id = Id::from_v7_bytes([42; 16]);
    assert_eq!(
        history.append(&JobSummary {
            job_id: id.clone(),
            kind_id: "scan.metadata".into(),
            kind_version: 1,
            scope_summary: "metadata: 1 root".into(),
            state: JobState::Done,
            counts: Default::default(),
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
            started_at_ms: Some(100),
            finished_at_ms: 200,
            correlation_id: id,
        }),
        Ok(HistoryAppend::Added),
    );
    drop(history);
    let reopened = open(dir.path())?;
    let settings = crate::settings::StartupSettings::load(&reopened, &["en", "ko"])?;
    assert_eq!(settings.scan_limit_source()().entries(), 200_000);

    let live = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let counts: (i64, i64, i64) = live.query_row(
        "SELECT (SELECT count(*) FROM job_runtime_job_record),
                (SELECT count(*) FROM job_runtime_job_link),
                (SELECT count(*) FROM user_env_recent_scope)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(counts, (1, 1, 0));
    let backups = dir.path().join("db-backups");
    assert_eq!(fs::read_dir(&backups)?.count(), 4);
    let original = Connection::open_with_flags(
        backups
            .join("backup-00000000000000000001")
            .join("database.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let saved: (u32, i64) = (
        original.pragma_query_value(None, "user_version", |row| row.get(0))?,
        original.query_row(
            "SELECT settings_version FROM user_env_setting_version",
            [],
            |row| row.get(0),
        )?,
    );
    assert_eq!(saved, (1, 7));
    Ok(())
}

#[test]
fn fresh_start_publishes_current_schema_without_creating_a_backup() -> TestResult {
    let dir = tempfile::tempdir()?;
    let opened = open_with_status(dir.path())?;
    assert!(opened.first_run);
    let reader = AppDb::open_read_only(&dir.path().join("diskunfold.db")).map_err(open_error)?;
    assert_eq!(reader.schema_version()?, HISTORY_SCHEMA_VERSION);
    assert!(!dir.path().join("db-backups").exists());
    drop(opened.database);
    Ok(())
}

#[test]
fn latest_schema_reopen_skips_managed_migration_and_keeps_backups_absent() -> TestResult {
    let dir = tempfile::tempdir()?;
    let opened = open_with_status(dir.path())?;
    assert!(opened.first_run);
    drop(opened.database);
    let path = dir.path().join("diskunfold.db");
    let before = AppDb::open_read_only(&path).map_err(open_error)?;
    assert_eq!(before.schema_version()?, HISTORY_SCHEMA_VERSION);
    drop(before);
    let backups = dir.path().join("db-backups");
    assert!(!backups.exists());

    let reopened = open_with_status(dir.path())?;
    assert!(!reopened.first_run);
    drop(reopened.database);
    let after = AppDb::open_read_only(&path).map_err(open_error)?;
    assert_eq!(after.schema_version()?, HISTORY_SCHEMA_VERSION);
    assert!(!backups.exists());
    Ok(())
}

#[test]
fn newer_and_malformed_databases_keep_startup_errors_and_files_unchanged() -> TestResult {
    let newer = tempfile::tempdir()?;
    drop(open(newer.path())?);
    let newer_path = newer.path().join("diskunfold.db");
    let changed = Connection::open(&newer_path)?;
    changed.pragma_update(None, "user_version", HISTORY_SCHEMA_VERSION + 1)?;
    drop(changed);
    let newer_before = fs::read(&newer_path)?;
    let newer_error = open(newer.path())
        .err()
        .ok_or("newer database was accepted")?;
    assert!(newer_error.to_string().contains("db.schema_mismatch"));
    assert_eq!(fs::read(&newer_path)?, newer_before);
    assert!(!newer.path().join("db-backups").exists());

    let tampered = tempfile::tempdir()?;
    drop(open(tampered.path())?);
    let tampered_path = tampered.path().join("diskunfold.db");
    let connection = Connection::open(&tampered_path)?;
    connection.execute_batch(
        "DROP TRIGGER app_db_schema_meta_no_update;
         UPDATE app_db_schema_meta SET definition_hash=zeroblob(32) WHERE migration_no=2;",
    )?;
    drop(connection);
    let tampered_before = fs::read(&tampered_path)?;
    let tampered_error = open(tampered.path())
        .err()
        .ok_or("tampered current database was accepted")?;
    assert!(tampered_error.to_string().contains("db.migration_tampered"));
    assert_eq!(fs::read(&tampered_path)?, tampered_before);
    assert!(!tampered.path().join("db-backups").exists());

    let malformed = tempfile::tempdir()?;
    let malformed_path = malformed.path().join("diskunfold.db");
    fs::write(&malformed_path, b"not a SQLite database")?;
    let malformed_before = fs::read(&malformed_path)?;
    let malformed_error = open(malformed.path())
        .err()
        .ok_or("malformed database file was accepted")?;
    assert!(malformed_error.to_string().contains("db.open_failed"));
    assert_eq!(fs::read(&malformed_path)?, malformed_before);
    assert!(!malformed.path().join("db-backups").exists());
    Ok(())
}

#[test]
fn backup_directory_failure_leaves_legacy_schema_and_existing_file_intact() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    drop(AppDb::create_gui(&path).map_err(open_error)?);
    let obstacle = dir.path().join("db-backups");
    fs::write(&obstacle, b"preserve")?;
    assert!(
        matches!(open(dir.path()), Err(error) if error.to_string().contains("db.backup_failed"))
    );
    assert_eq!(fs::read(&obstacle)?, b"preserve");
    assert_eq!(
        AppDb::open_read_only(&path)
            .map_err(open_error)?
            .schema_version()?,
        1
    );
    Ok(())
}

#[test]
fn startup_recovers_committed_interruption_once_and_retries_only_on_reopen() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    drop(open(dir.path())?);
    let fixture = Connection::open(&path)?;
    fixture.execute_batch(
        "INSERT INTO job_runtime_job_start
           (job_id, kind_id, kind_version, scope_summary, started_at_ms, record_origin, created_at_ms)
         VALUES (X'01010101010171018101010101010101', 'change.trash', 1, 'redacted', 80, 'this_app', 80);
         CREATE TRIGGER fail_link BEFORE INSERT ON job_runtime_job_link
         BEGIN SELECT RAISE(ABORT, 'fixture'); END;",
    )?;

    let first = open(dir.path())?;
    let events = DesktopEvents::default();
    assert!(events.recover_history(&first, 100).is_err());
    drop(first);
    assert_eq!(interruption_counts(&fixture)?, (1, 0, 0, 0, 0));

    fixture.execute_batch("DROP TRIGGER fail_link")?;
    let reopened = open(dir.path())?;
    events.recover_history(&reopened, 200)?;
    assert_eq!(interruption_counts(&fixture)?, (1, 1, 1, 0, 1));
    let detected: i64 = fixture.query_row(
        "SELECT detected_at_ms FROM job_runtime_job_interrupt",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(detected, 200);
    drop(reopened);

    let reopened = open(dir.path())?;
    events.recover_history(&reopened, 300)?;
    assert_eq!(interruption_counts(&fixture)?, (1, 1, 1, 0, 1));
    Ok(())
}

fn interruption_counts(db: &Connection) -> rusqlite::Result<(i64, i64, i64, i64, i64)> {
    db.query_row(
        "SELECT (SELECT count(*) FROM job_runtime_job_start),
                (SELECT count(*) FROM job_runtime_job_interrupt),
                (SELECT count(*) FROM job_runtime_job_link),
                (SELECT count(*) FROM job_runtime_job_record),
                last_record_seq
         FROM job_runtime_history_state",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )
}

#[test]
fn startup_upgrades_v4_to_v5_and_preserves_existing_settings() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    drop(AppDb::create_gui(&path).map_err(open_error)?);
    for version in 2..=4 {
        let backup = dir.path().join(format!("initial-{version}.sqlite"));
        assert_eq!(
            du_app_db::migration::apply_next(&path, &backup, &HISTORY_DEFINITIONS)
                .map_err(migration_error)?,
            version
        );
    }
    let connection = Connection::open(&path)?;
    connection.execute_batch("INSERT INTO user_env_setting VALUES ('space.shared_table_max_entries', '200000', 'screen', 1, 2, 1); UPDATE user_env_setting_version SET settings_version=7;")?;
    drop(connection);
    let database = open(dir.path())?;
    let settings = crate::settings::StartupSettings::load(&database, &["en", "ko"])?;
    assert_eq!(settings.scan_limit_source()().entries(), 200_000);
    let reader = AppDb::open_read_only(&path).map_err(open_error)?;
    assert_eq!(reader.schema_version()?, 5);
    assert_eq!(fs::read_dir(dir.path().join("db-backups"))?.count(), 1);
    assert!(
        reader
            .latest_ruleset()
            .map_err(|e| io::Error::other(format!("{e:?}")))?
            .is_none()
    );
    Ok(())
}
