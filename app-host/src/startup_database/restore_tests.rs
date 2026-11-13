//! FR8.HD1.app-host.DD1 / FR8.HD1.app-db.DD1: startup restores the pre-migration backup.
use super::*;
use rusqlite::{Connection, OpenFlags};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A schema 2 database whose own table blocks migration step 3, plus a row that must survive.
fn blocked_database(dir: &Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let path = dir.join(DATABASE_FILE);
    drop(AppDb::create_gui(&path).map_err(open_error)?);
    du_app_db::migration::apply_next(
        &path,
        &dir.join("ordinary-backup.sqlite"),
        &HISTORY_DEFINITIONS,
    )
    .map_err(|error| format!("{error:?}"))?;
    let raw = Connection::open(&path)?;
    raw.execute_batch(
        "INSERT INTO user_env_setting VALUES ('locale.display_language', '\"ko\"', 'screen', 1, 2, 1);
         CREATE TABLE job_runtime_job_link (note TEXT NOT NULL);
         INSERT INTO job_runtime_job_link VALUES ('older row');",
    )?;
    Ok(path)
}

fn rows(path: &Path) -> Result<(u32, String, String), Box<dyn std::error::Error>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok((
        db.pragma_query_value(None, "user_version", |row| row.get(0))?,
        db.query_row("SELECT value_json FROM user_env_setting", [], |row| {
            row.get(0)
        })?,
        db.query_row("SELECT note FROM job_runtime_job_link", [], |row| {
            row.get(0)
        })?,
    ))
}

#[test]
fn failed_migration_restores_the_backup_and_blocks_with_the_startup_reason() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = blocked_database(dir.path())?;
    let before = rows(&path)?;
    assert_eq!(before.0, 2);

    let error = open(dir.path())
        .err()
        .ok_or("failing migration was accepted")?;
    let message = error.to_string();
    assert_eq!(message, "startup.migration_failed: restored");
    assert_eq!(rows(&path)?, before);
    let backups = dir.path().join("db-backups");
    assert!(backups.join("backup-00000000000000000001").is_dir());
    assert!(backups.join("restored-00000000000000000001.json").is_file());
    assert!(!backups.join("migration-journal.json").exists());
    // The lock was released, so the next start can try again (and fails the same way).
    assert!(
        matches!(open(dir.path()), Err(error) if error.to_string().contains("startup.migration_failed"))
    );
    Ok(())
}

#[test]
fn interrupted_restore_is_finished_before_anything_reads_the_database() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = blocked_database(dir.path())?;
    let before = rows(&path)?;
    drop(
        open(dir.path())
            .err()
            .ok_or("failing migration was accepted")?,
    );
    let backups = dir.path().join("db-backups");
    // Put the product's own restore record back as an open journal and damage the database.
    fs::rename(
        backups.join("restored-00000000000000000001.json"),
        backups.join("migration-journal.json"),
    )?;
    let raw = Connection::open(&path)?;
    raw.execute_batch(
        "INSERT INTO job_runtime_job_link VALUES ('half migrated'); PRAGMA user_version = 3;",
    )?;
    drop(raw);

    let error = open(dir.path())
        .err()
        .ok_or("interrupted state was opened")?;
    assert_eq!(error.to_string(), "startup.migration_failed: restored");
    assert_eq!(rows(&path)?, before);
    assert!(!backups.join("migration-journal.json").exists());
    Ok(())
}

#[test]
fn failed_restore_stops_startup_and_keeps_database_backups_and_journal() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = blocked_database(dir.path())?;
    drop(
        open(dir.path())
            .err()
            .ok_or("failing migration was accepted")?,
    );
    let backups = dir.path().join("db-backups");
    fs::rename(
        backups.join("restored-00000000000000000001.json"),
        backups.join("migration-journal.json"),
    )?;
    let copy = backups
        .join("backup-00000000000000000001")
        .join("database.sqlite");
    let mut damaged = fs::read(&copy)?;
    damaged[100] ^= 0xff;
    fs::write(&copy, &damaged)?;
    let database_before = rows(&path)?;

    let error = open(dir.path())
        .err()
        .ok_or("restore from a damaged backup was accepted")?;
    let message = error.to_string();
    assert_eq!(message, "startup.migration_failed: restore_failed");
    assert!(!message.contains("db.restore_failed"), "{message}");
    assert_eq!(rows(&path)?, database_before);
    assert_eq!(fs::read(&copy)?, damaged);
    assert!(backups.join("migration-journal.json").is_file());
    Ok(())
}

#[test]
fn journal_probe_fails_closed_on_every_error_except_not_found() {
    let present = fs::metadata(std::env::temp_dir());
    assert!(matches!(journal_pending(present), Ok(true)));
    assert!(matches!(
        journal_pending(Err(io::ErrorKind::NotFound.into())),
        Ok(false)
    ));
    for kind in [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::TimedOut,
        io::ErrorKind::Other,
    ] {
        let error = journal_pending(Err(kind.into()))
            .err()
            .map(|error| error.to_string());
        assert_eq!(
            error.as_deref(),
            Some("startup.migration_failed: journal_unreadable"),
            "{kind:?}"
        );
    }
}

#[test]
fn journal_that_cannot_be_read_blocks_startup_and_is_preserved() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = blocked_database(dir.path())?;
    let before = rows(&path)?;
    let backups = dir.path().join("db-backups");
    fs::create_dir(&backups)?;
    // A directory in the journal's place cannot be parsed; startup must not migrate or restore.
    fs::create_dir(backups.join("migration-journal.json"))?;

    let error = open(dir.path())
        .err()
        .ok_or("unreadable journal was accepted")?;
    assert_eq!(
        error.to_string(),
        "startup.migration_failed: restore_failed"
    );
    assert_eq!(rows(&path)?, before);
    assert!(backups.join("migration-journal.json").is_dir());
    assert!(!backups.join("backup-00000000000000000001").exists());
    Ok(())
}
