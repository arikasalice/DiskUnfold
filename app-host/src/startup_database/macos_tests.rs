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
fn older_macos_database_is_preserved_and_startup_stops_without_migrating() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
    drop(AppDb::create_gui(&path).map_err(open_error)?);
    let before = fs::read(&path)?;

    let error = open(dir.path())
        .err()
        .ok_or("older database was accepted")?;
    assert!(error.to_string().contains("startup.migration_failed"));
    assert!(error.to_string().contains("is preserved"));
    assert_eq!(fs::read(&path)?, before);
    assert_eq!(
        AppDb::probe_schema_version_read_only(&path).map_err(open_error)?,
        1
    );
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
