use super::*;
use du_core_contracts::{
    Id,
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
};
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
    let counts: (i64, i64) = live.query_row(
        "SELECT (SELECT count(*) FROM job_runtime_job_record),
                (SELECT count(*) FROM job_runtime_job_link)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(counts, (1, 1));
    let backups = dir.path().join("db-backups");
    assert_eq!(fs::read_dir(&backups)?.count(), 2);
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
    let database = open(dir.path())?;
    let reader = AppDb::open_read_only(&dir.path().join("diskunfold.db")).map_err(open_error)?;
    assert_eq!(reader.schema_version()?, HISTORY_SCHEMA_VERSION);
    assert!(!dir.path().join("db-backups").exists());
    drop(database);
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
