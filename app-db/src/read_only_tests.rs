//! `AppDb::open_read_only` never creates or modifies files, even for WAL databases.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::{AppDb, OpenError};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type DirectoryState = Vec<(OsString, u64, Vec<u8>, SystemTime)>;

/// Names, lengths, bytes and modification times of every entry in `directory`.
/// The shared-memory file is mapped and byte-range locked by SQLite on Windows,
/// so its bytes are not read there; its length and time still are.
fn directory_state(directory: &Path) -> Result<DirectoryState, Box<dyn std::error::Error>> {
    let mut state = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let bytes = if name.to_string_lossy().ends_with("-shm") {
            Vec::new()
        } else {
            fs::read(entry.path())?
        };
        let metadata = entry.metadata()?;
        state.push((name, metadata.len(), bytes, metadata.modified()?));
    }
    state.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(state)
}

fn names(state: &DirectoryState) -> Vec<String> {
    state
        .iter()
        .map(|entry| entry.0.to_string_lossy().into_owned())
        .collect()
}

fn side(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn rows(db: &AppDb) -> Result<i64, rusqlite::Error> {
    db.connection
        .query_row("SELECT count(*) FROM read_only_probe", [], |row| row.get(0))
}

/// A WAL database with a table of `count` rows, written by an open connection.
fn writer_with_rows(path: &Path, count: i64) -> Result<AppDb, Box<dyn std::error::Error>> {
    let writer = AppDb::create_gui(path).map_err(|error| format!("{error:?}"))?;
    writer
        .connection
        .execute_batch("CREATE TABLE read_only_probe(value INTEGER NOT NULL)")?;
    for value in 0..count {
        writer
            .connection
            .execute("INSERT INTO read_only_probe(value) VALUES (?1)", [value])?;
    }
    Ok(writer)
}

#[test]
fn wal_database_with_live_side_files_is_read_without_touching_any_file() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("live.db");
    let writer = writer_with_rows(&path, 3)?;
    assert!(fs::metadata(side(&path, "-wal"))?.len() > 0);
    assert!(side(&path, "-shm").exists());
    let before = directory_state(dir.path())?;

    let reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(rows(&reader)?, 3, "uncheckpointed commits are visible");
    writer
        .connection
        .execute("INSERT INTO read_only_probe(value) VALUES (99)", [])?;
    assert_eq!(rows(&reader)?, 4, "a live reader follows the writer");
    drop(reader);

    let after = directory_state(dir.path())?;
    assert_eq!(names(&after), names(&before));
    // The writer's own insert appended to the log; the main file must be unchanged.
    assert_eq!(after[0].0, "live.db");
    assert_eq!(after[0], before[0]);
    Ok(())
}

#[test]
fn wal_database_without_side_files_is_read_from_a_snapshot_and_creates_nothing() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("closed.db");
    drop(writer_with_rows(&path, 5)?);
    assert!(!side(&path, "-wal").exists(), "clean close removes the log");
    assert!(!side(&path, "-shm").exists());
    let before = directory_state(dir.path())?;

    let reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(rows(&reader)?, 5);
    assert_eq!(AppDb::probe_schema_version_read_only(&path).ok(), Some(1));
    assert!(
        reader
            .connection
            .execute("DELETE FROM read_only_probe", [])
            .is_err()
    );
    drop(reader);

    assert_eq!(directory_state(dir.path())?, before);
    Ok(())
}

#[test]
fn snapshot_of_a_closed_wal_database_does_not_follow_later_commits() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("closed.db");
    drop(writer_with_rows(&path, 2)?);
    let snapshot = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;

    let writer = AppDb::open_gui(&path).map_err(|error| format!("{error:?}"))?;
    writer
        .connection
        .execute("INSERT INTO read_only_probe(value) VALUES (7)", [])?;

    assert_eq!(rows(&snapshot)?, 2);
    let fresh = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(rows(&fresh)?, 3);
    Ok(())
}

#[test]
fn reading_while_another_connection_has_an_open_write_transaction_sees_only_commits() -> TestResult
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("busy.db");
    let mut writer = writer_with_rows(&path, 4)?;
    let transaction = writer.connection.transaction()?;
    transaction.execute("INSERT INTO read_only_probe(value) VALUES (100)", [])?;
    let before = directory_state(dir.path())?;

    let reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(rows(&reader)?, 4, "uncommitted rows stay invisible");
    drop(reader);

    assert_eq!(names(&directory_state(dir.path())?), names(&before));
    assert_eq!(directory_state(dir.path())?[0], before[0]);
    transaction.commit()?;
    Ok(())
}

#[test]
fn wal_with_frames_but_no_shared_memory_file_is_not_readable_and_creates_nothing() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("source.db");
    let writer = writer_with_rows(&path, 3)?;
    // A writer that died: main file and log copied, no shared-memory file.
    let crashed = dir.path().join("crashed");
    fs::create_dir(&crashed)?;
    let copy = crashed.join("source.db");
    fs::copy(&path, &copy)?;
    fs::copy(side(&path, "-wal"), side(&copy, "-wal"))?;
    drop(writer);
    let before = directory_state(&crashed)?;

    assert!(matches!(
        AppDb::open_read_only(&copy),
        Err(OpenError::ReadOnlyUnavailable)
    ));
    assert!(matches!(
        AppDb::probe_schema_version_read_only(&copy),
        Err(OpenError::ReadOnlyUnavailable)
    ));

    assert_eq!(directory_state(&crashed)?, before);
    assert!(!side(&copy, "-shm").exists());
    Ok(())
}

#[test]
fn missing_database_is_not_created_and_rollback_journal_database_is_untouched() -> TestResult {
    let dir = tempfile::tempdir()?;
    assert!(AppDb::open_read_only(&dir.path().join("missing.db")).is_err());
    assert!(directory_state(dir.path())?.is_empty());

    let path = dir.path().join("delete.db");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(&format!(
        "PRAGMA journal_mode=DELETE; PRAGMA application_id={}; PRAGMA user_version=1;
         CREATE TABLE read_only_probe(value INTEGER NOT NULL); INSERT INTO read_only_probe VALUES (1);",
        super::APPLICATION_ID
    ))?;
    drop(connection);
    let before = directory_state(dir.path())?;
    let reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(rows(&reader)?, 1);
    drop(reader);
    assert_eq!(directory_state(dir.path())?, before);
    Ok(())
}
