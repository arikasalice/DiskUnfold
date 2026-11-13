use super::*;
use rusqlite::Connection;
use std::os::unix::fs::PermissionsExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn new_macos_data_directories_are_private_and_existing_modes_are_preserved() -> TestResult {
    let root = tempfile::tempdir()?;
    let created = root.path().join("DiskUnfold");
    drop(open(&created)?);
    assert_eq!(fs::metadata(&created)?.permissions().mode() & 0o777, 0o700);

    let existing = root.path().join("Existing");
    fs::create_dir(&existing)?;
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o750))?;
    drop(open(&existing)?);
    assert_eq!(fs::metadata(&existing)?.permissions().mode() & 0o777, 0o750);
    Ok(())
}

#[test]
fn fresh_macos_start_publishes_current_schema_and_holds_the_writer_lock() -> TestResult {
    let dir = tempfile::tempdir()?;
    let opened = open_with_status(dir.path())?;
    assert!(opened.first_run);
    let path = dir.path().join(DATABASE_FILE);
    let reader = AppDb::open_read_only(&path).map_err(open_error)?;
    assert_eq!(reader.schema_version()?, HISTORY_SCHEMA_VERSION);
    assert!(!dir.path().join("db-backups").exists());
    assert!(matches!(
        open(dir.path()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists
    ));
    drop(reader);
    drop(opened.database);

    let reopened = open_with_status(dir.path())?;
    assert!(!reopened.first_run);
    drop(reopened.database);
    Ok(())
}

#[test]
fn existing_current_macos_database_opens_without_being_classified_as_first_run() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
    drop(AppDb::create_gui_with_history(&path).map_err(open_error)?);
    let before_version = AppDb::probe_schema_version_read_only(&path).map_err(open_error)?;
    assert_eq!(before_version, HISTORY_SCHEMA_VERSION);

    let opened = open_with_status(dir.path())?;
    assert!(!opened.first_run);
    let reader = AppDb::open_read_only(&path).map_err(open_error)?;
    assert_eq!(reader.schema_version()?, HISTORY_SCHEMA_VERSION);
    drop(reader);
    drop(opened.database);
    Ok(())
}

#[test]
fn older_macos_database_is_backed_up_and_migrated_by_the_shared_path() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
    drop(AppDb::create_gui(&path).map_err(open_error)?);

    drop(open(dir.path())?);
    assert_eq!(
        AppDb::probe_schema_version_read_only(&path).map_err(open_error)?,
        HISTORY_SCHEMA_VERSION
    );
    let backups = dir.path().join("db-backups");
    assert!(backups.join("backup-00000000000000000001").is_dir());
    assert!(!backups.join("migration-journal.json").exists());
    Ok(())
}

#[test]
fn older_macos_database_with_wal_rows_is_migrated_and_keeps_the_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
    drop(AppDb::create_gui(&path).map_err(open_error)?);

    // Keep the writer open with automatic checkpoints off, so the committed row lives only in
    // the WAL. The row goes into an existing version 1 table; a foreign table would make the
    // baseline unrecognized.
    let writer = Connection::open(&path)?;
    writer.pragma_update(None, "wal_autocheckpoint", 0)?;
    writer.execute_batch(
        "INSERT INTO user_env_setting VALUES ('locale.display_language', '\"ko\"', 'screen', 1, 2, 1);",
    )?;
    let wal = path.with_extension("db-wal");
    assert!(wal.metadata()?.len() > 32);

    drop(open(dir.path())?);
    drop(writer);
    let reader = Connection::open(&path)?;
    assert_eq!(
        reader.query_row(
            "SELECT value_json FROM user_env_setting WHERE setting_key = 'locale.display_language'",
            [],
            |row| row.get::<_, String>(0)
        )?,
        "\"ko\""
    );
    assert_eq!(
        reader.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))?,
        HISTORY_SCHEMA_VERSION
    );
    Ok(())
}

#[test]
fn corrupt_macos_database_is_reported_and_preserved() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
    let bytes = b"not a SQLite database";
    fs::write(&path, bytes)?;

    let error = open(dir.path())
        .err()
        .ok_or("corrupt database was accepted")?;
    assert!(error.to_string().contains("db.corrupt"));
    assert_eq!(fs::read(&path)?, bytes);
    assert!(!dir.path().join("db-backups").exists());
    Ok(())
}

#[test]
fn newer_macos_database_is_preserved_and_refused() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
    drop(AppDb::create_gui_with_history(&path).map_err(open_error)?);
    let raw = Connection::open(&path)?;
    raw.pragma_update(None, "user_version", HISTORY_SCHEMA_VERSION + 1)?;
    drop(raw);
    let before = fs::read(&path)?;

    let error = open(dir.path())
        .err()
        .ok_or("newer database was accepted")?;
    assert!(error.to_string().contains("db.schema_mismatch"));
    assert_eq!(fs::read(&path)?, before);
    assert_eq!(
        AppDb::probe_schema_version_read_only(&path).map_err(open_error)?,
        HISTORY_SCHEMA_VERSION + 1
    );
    Ok(())
}

#[test]
fn startup_observes_newer_version_in_uncheckpointed_wal_and_preserves_database_files() -> TestResult
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
    drop(AppDb::create_gui_with_history(&path).map_err(open_error)?);

    // Keep the writer open and disable automatic checkpoints so user_version remains only in
    // the latest committed WAL frame while the main database header keeps the current version.
    let writer = Connection::open(&path)?;
    writer.pragma_update(None, "wal_autocheckpoint", 0)?;
    let newer = HISTORY_SCHEMA_VERSION + 7;
    writer.pragma_update(None, "user_version", newer)?;
    let wal = path.with_extension("db-wal");
    let shm = path.with_extension("db-shm");
    assert!(wal.metadata()?.len() > 32);
    let header = fs::read(&path)?;
    assert_eq!(
        u32::from_be_bytes(header[60..64].try_into()?),
        HISTORY_SCHEMA_VERSION
    );

    assert_eq!(
        AppDb::probe_schema_version_read_only(&path).map_err(open_error)?,
        newer
    );

    let main_before = fs::read(&path)?;
    let wal_before = fs::read(&wal)?;
    let shm_before = fs::read(&shm)?;
    let error = open(dir.path())
        .err()
        .ok_or("newer WAL schema was accepted")?;
    assert!(error.to_string().contains("db.schema_mismatch"));
    assert_eq!(fs::read(&path)?, main_before);
    assert_eq!(fs::read(&wal)?, wal_before);
    assert_eq!(shm.metadata()?.len(), shm_before.len() as u64);
    assert_eq!(
        writer.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))?,
        newer
    );
    writer.pragma_update(None, "user_version", newer + 1)?;
    assert_eq!(
        AppDb::probe_schema_version_read_only(&path).map_err(open_error)?,
        newer + 1
    );
    drop(writer);
    Ok(())
}
