use super::*;
use rusqlite::Connection;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn fresh_linux_start_publishes_current_schema_and_holds_the_writer_lock() -> TestResult {
    let dir = tempfile::tempdir()?;
    let database = open(dir.path())?;
    let reader = AppDb::open_read_only(&dir.path().join(DATABASE_FILE)).map_err(open_error)?;
    assert_eq!(reader.schema_version()?, HISTORY_SCHEMA_VERSION);
    assert!(!dir.path().join("db-backups").exists());
    assert!(matches!(open(dir.path()), Err(error) if error.kind() == io::ErrorKind::AlreadyExists));
    drop(reader);
    drop(database);
    assert!(open(dir.path()).is_ok());
    Ok(())
}

#[test]
fn linux_start_migrates_legacy_database_only_after_managed_backup() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
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
    assert_eq!(settings.stored_language()?, None);
    assert_eq!(settings.scan_limit_source()().entries(), 200_000);
    assert_eq!(fs::read_dir(dir.path().join("db-backups"))?.count(), 4);
    let reader = AppDb::open_read_only(&path).map_err(open_error)?;
    assert_eq!(reader.schema_version()?, HISTORY_SCHEMA_VERSION);
    drop(database);
    Ok(())
}

#[test]
fn linux_migration_rejects_symlink_backup_directory_without_touching_database() -> TestResult {
    let data_dir = tempfile::tempdir()?;
    let external = tempfile::tempdir()?;
    let path = data_dir.path().join(DATABASE_FILE);
    drop(AppDb::create_gui(&path).map_err(open_error)?);
    let before = fs::read(&path)?;
    std::os::unix::fs::symlink(external.path(), data_dir.path().join("db-backups"))?;

    let error = open(data_dir.path())
        .err()
        .ok_or("symlink backup path accepted")?;
    assert!(error.to_string().contains("db.backup_failed"));
    assert_eq!(fs::read(&path)?, before);
    assert_eq!(fs::read_dir(external.path())?.count(), 0);
    Ok(())
}

#[test]
fn linux_migration_rejects_regular_file_backup_obstacle() -> TestResult {
    let data_dir = tempfile::tempdir()?;
    let path = data_dir.path().join(DATABASE_FILE);
    drop(AppDb::create_gui(&path).map_err(open_error)?);
    let backups = data_dir.path().join("db-backups");
    fs::write(&backups, b"preserve")?;
    let database_before = fs::read(&path)?;
    let error = open(data_dir.path())
        .err()
        .ok_or("file backup path accepted")?;
    assert!(error.to_string().contains("db.backup_failed"));
    assert_eq!(fs::read(&path)?, database_before);
    assert_eq!(fs::read(&backups)?, b"preserve");
    Ok(())
}
