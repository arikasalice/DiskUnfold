use std::{
    collections::BTreeMap,
    error::Error,
    ffi::OsString,
    fs,
    path::Path,
    sync::{Arc, Barrier},
    thread,
};

use du_core_contracts::{
    Id,
    job::{HistoryAppend, JobState, JobSummary},
};
use rusqlite::{Connection, TransactionBehavior};
use tempfile::tempdir;

use super::{HistoryOpenError, HistoryOpenRefusal};
use crate::{AppDb, HISTORY_SCHEMA_VERSION};

type TestResult = Result<(), Box<dyn Error>>;

fn summary(id: u8, finished_at_ms: i64) -> JobSummary {
    let job_id = Id::from_v7_bytes([id; 16]);
    JobSummary {
        job_id: job_id.clone(),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: Default::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(finished_at_ms - 1),
        finished_at_ms,
        correlation_id: job_id,
    }
}

fn create_current(path: &Path) -> Result<(), Box<dyn Error>> {
    drop(
        AppDb::create_gui_with_history(path)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?,
    );
    Ok(())
}

fn directory_snapshot(directory: &Path) -> Result<BTreeMap<OsString, Vec<u8>>, Box<dyn Error>> {
    let mut entries = BTreeMap::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            entries.insert(entry.file_name(), fs::read(entry.path())?);
        }
    }
    Ok(entries)
}

fn assert_refused_unchanged(
    path: &Path,
    allow_wal_side_files: bool,
    is_expected: impl FnOnce(&HistoryOpenError) -> bool,
) -> TestResult {
    let directory = path.parent().ok_or("database path has no parent")?;
    let before = directory_snapshot(directory)?;
    let main_before = fs::read(path).ok();
    let result = AppDb::open_existing_for_history(path);
    let error = match result {
        Err(error) => error,
        Ok(database) => {
            drop(database);
            return Err("database was unexpectedly accepted".into());
        }
    };
    assert!(is_expected(&error), "unexpected refusal: {error:?}");
    let after = directory_snapshot(directory)?;
    assert_eq!(fs::read(path).ok(), main_before, "main DB bytes changed");
    for (name, bytes) in &before {
        let is_wal_side =
            name.to_string_lossy().ends_with("-wal") || name.to_string_lossy().ends_with("-shm");
        if !allow_wal_side_files || !is_wal_side {
            assert_eq!(
                after.get(name),
                Some(bytes),
                "existing file changed: {name:?}"
            );
        }
    }
    for name in after.keys().filter(|name| !before.contains_key(*name)) {
        let is_wal_side =
            name.to_string_lossy().ends_with("-wal") || name.to_string_lossy().ends_with("-shm");
        assert!(
            allow_wal_side_files && is_wal_side,
            "unexpected new file: {name:?}"
        );
    }
    Ok(())
}

#[test]
fn a_missing_database_is_refused_without_creating_a_file() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    assert_refused_unchanged(&path, false, |error| {
        matches!(
            error,
            HistoryOpenError::Refused(HistoryOpenRefusal::MissingDatabase)
        )
    })
}

#[test]
fn older_and_newer_schemas_are_refused_without_changing_the_database() -> TestResult {
    for version in [HISTORY_SCHEMA_VERSION - 1, HISTORY_SCHEMA_VERSION + 1] {
        let directory = tempdir()?;
        let path = directory.path().join("diskunfold.db");
        create_current(&path)?;
        Connection::open(&path)?.pragma_update(None, "user_version", version)?;
        assert_refused_unchanged(&path, true, |error| {
            matches!(
                error,
                HistoryOpenError::Refused(HistoryOpenRefusal::UnsupportedSchema(actual))
                    if *actual == version
            )
        })?;
    }
    Ok(())
}

#[test]
fn an_incomplete_creation_marker_is_refused_without_modifying_files() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    create_current(&path)?;
    fs::write(path.with_file_name("diskunfold.db.creating"), b"")?;
    assert_refused_unchanged(&path, true, |error| {
        matches!(
            error,
            HistoryOpenError::Refused(HistoryOpenRefusal::MigrationInProgress)
        )
    })
}

#[test]
fn a_managed_migration_journal_is_refused_without_modifying_files() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    create_current(&path)?;
    let backup_directory = directory.path().join("db-backups");
    fs::create_dir(&backup_directory)?;
    let journal = backup_directory.join(crate::migration::managed::JOURNAL);
    fs::write(&journal, b"migration pending")?;
    assert_refused_unchanged(&path, true, |error| {
        matches!(
            error,
            HistoryOpenError::Refused(HistoryOpenRefusal::MigrationInProgress)
        )
    })?;
    assert_eq!(fs::read(&journal)?, b"migration pending");
    assert_eq!(fs::read_dir(backup_directory)?.count(), 1);
    Ok(())
}

#[test]
fn a_rollback_journal_database_is_refused_without_side_files() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    create_current(&path)?;
    let connection = Connection::open(&path)?;
    let journal: String =
        connection.pragma_update_and_check(None, "journal_mode", "DELETE", |r| r.get(0))?;
    assert_eq!(journal.to_lowercase(), "delete");
    drop(connection);
    assert_refused_unchanged(&path, false, |error| {
        matches!(error, HistoryOpenError::Refused(HistoryOpenRefusal::NotWal))
    })
}

#[test]
fn a_foreign_application_id_is_refused_without_changing_the_database() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    create_current(&path)?;
    Connection::open(&path)?.pragma_update(None, "application_id", 0x1234_u32)?;
    assert_refused_unchanged(&path, true, |error| {
        matches!(
            error,
            HistoryOpenError::Refused(HistoryOpenRefusal::ForeignDatabase)
        )
    })
}

#[test]
fn an_existing_current_database_appends_once_and_deduplicates_by_job_id() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    create_current(&path)?;
    let mut database = AppDb::open_existing_for_history(&path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let job = summary(7, 700);
    assert_eq!(
        database.append_scan_history(&job, 701),
        Ok(HistoryAppend::Added)
    );
    assert_eq!(
        database.append_scan_history(&job, 702),
        Ok(HistoryAppend::Duplicate)
    );
    let (records, links, sequence): (i64, i64, i64) = database.connection.query_row(
        "SELECT (SELECT count(*) FROM job_runtime_job_record),
                (SELECT count(*) FROM job_runtime_job_link),
                last_record_seq FROM job_runtime_history_state WHERE state_key='history'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!((records, links, sequence), (1, 1, 1));
    Ok(())
}

#[test]
fn commit_rechecks_schema_and_migration_journal_before_writing() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    create_current(&path)?;
    let mut database = AppDb::open_existing_for_history(&path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    Connection::open(&path)?.pragma_update(None, "user_version", HISTORY_SCHEMA_VERSION + 1)?;
    assert_eq!(
        database.append_scan_history(&summary(8, 800), 801),
        Err("history.schema_changed".into())
    );
    drop(database);

    Connection::open(&path)?.pragma_update(None, "user_version", HISTORY_SCHEMA_VERSION)?;
    let mut database = AppDb::open_existing_for_history(&path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let backup_directory = directory.path().join("db-backups");
    fs::create_dir(&backup_directory)?;
    fs::write(
        backup_directory.join(crate::migration::managed::JOURNAL),
        b"migration pending",
    )?;
    assert_eq!(
        database.append_scan_history(&summary(9, 900), 901),
        Err("history.migration_in_progress".into())
    );
    let count: i64 = database.connection.query_row(
        "SELECT count(*) FROM job_runtime_job_record WHERE job_id = ?1",
        [summary(9, 900)
            .job_id
            .v7_bytes()
            .ok_or("invalid test id")?
            .as_slice()],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
fn gui_and_cli_connections_append_without_losing_rows_or_sequences() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    create_current(&path)?;
    let gui = AppDb::open_gui_with_history(&path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let cli = AppDb::open_existing_for_history(&path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let barrier = Arc::new(Barrier::new(3));
    let gui_barrier = Arc::clone(&barrier);
    let cli_barrier = Arc::clone(&barrier);
    let gui_thread = thread::spawn(move || {
        let mut gui = gui;
        gui_barrier.wait();
        gui.append_scan_history(&summary(10, 1000), 1001)
    });
    let cli_thread = thread::spawn(move || {
        let mut cli = cli;
        cli_barrier.wait();
        cli.append_scan_history(&summary(11, 1100), 1101)
    });
    barrier.wait();
    let gui_result = gui_thread
        .join()
        .map_err(|_| std::io::Error::other("GUI test thread panicked"))?;
    let cli_result = cli_thread
        .join()
        .map_err(|_| std::io::Error::other("CLI test thread panicked"))?;
    assert_eq!(gui_result, Ok(HistoryAppend::Added));
    assert_eq!(cli_result, Ok(HistoryAppend::Added));

    let connection = Connection::open(&path)?;
    let (rows, unique_jobs, last_sequence): (i64, i64, i64) = connection.query_row(
        "SELECT count(*), count(DISTINCT job_id), max(record_seq) FROM job_runtime_job_link",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!((rows, unique_jobs, last_sequence), (2, 2, 2));
    let mut statement =
        connection.prepare("SELECT record_seq FROM job_runtime_job_link ORDER BY record_seq")?;
    let sequences = statement
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert_eq!(sequences, [1, 2]);
    Ok(())
}

#[test]
fn a_contended_append_returns_busy_after_the_five_second_timeout() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    create_current(&path)?;
    let mut blocker = Connection::open(&path)?;
    blocker.busy_timeout(crate::BUSY_TIMEOUT)?;
    let _transaction = blocker.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut database = AppDb::open_existing_for_history(&path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    assert_eq!(
        database.append_scan_history(&summary(12, 1200), 1201),
        Err("db.busy".into())
    );
    Ok(())
}

#[test]
fn interruptible_busy_wait_stops_without_appending() -> TestResult {
    let directory = tempdir()?;
    let path = directory.path().join("diskunfold.db");
    create_current(&path)?;
    let mut blocker = Connection::open(&path)?;
    let _transaction = blocker.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut database = AppDb::open_existing_for_history(&path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    assert_eq!(
        database.append_scan_history_interruptible(&summary(13, 1300), 1301, || true),
        Err("history.interrupted".into())
    );
    let count: i64 = database.connection.query_row(
        "SELECT count(*) FROM job_runtime_job_record",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}
