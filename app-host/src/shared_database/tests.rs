use super::*;
use du_core_contracts::{
    Id,
    job::{JobCounts, JobState},
    settings::{SettingsCommit, SettingsSource},
};
use du_user_env::{Registry, settings::SettingsService, space};
use rusqlite::Connection;
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn summary() -> JobSummary {
    JobSummary {
        job_id: Id::from_v7_bytes([1; 16]),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: JobCounts::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: Some(Id::from_v7_bytes([2; 16])),
        started_at_ms: Some(100),
        finished_at_ms: 200,
        correlation_id: Id::from_v7_bytes([3; 16]),
    }
}

fn database(path: &std::path::Path) -> Result<SharedDatabase, Box<dyn std::error::Error>> {
    let db = AppDb::create_gui_with_history(path).map_err(|e| format!("{e:?}"))?;
    Ok(SharedDatabase::new(db, None))
}

fn restore(
    store: SharedSettings,
) -> Result<SettingsService<SharedSettings>, Box<dyn std::error::Error>> {
    let registry = Registry::new(space::descriptors()).map_err(|e| format!("{e:?}"))?;
    Ok(SettingsService::restore(
        registry,
        store,
        SharedSettings::load_for_restore,
        |store, revision, invalid| store.quarantine(revision, invalid, 100),
    ))
}

fn commit(
    store: &mut SharedSettings,
    revision: u64,
    value: i64,
) -> Result<u64, Box<dyn std::error::Error>> {
    store
        .commit(
            revision,
            &SettingsChanges::from([(space::SHARED_TABLE_MAX_ENTRIES.into(), Some(json!(value)))]),
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 120,
            },
        )
        .map_err(|e| format!("{e:?}").into())
}

#[test]
fn settings_and_history_use_one_handle() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("shared.db");
    let db = database(&path)?;
    let mut settings = db.settings();
    let history = db.history(|| 250);
    assert!(Arc::ptr_eq(&settings.0.0, &history.database.0));
    assert_eq!(commit(&mut settings, 0, 200_000)?, 1);
    let restored = restore(db.settings())?;
    assert_eq!(
        restored
            .snapshot()
            .values
            .get(space::SHARED_TABLE_MAX_ENTRIES),
        Some(&json!(200_000))
    );
    assert_eq!(history.append(&summary()), Ok(HistoryAppend::Added));
    let sql = Connection::open(path)?;
    let (records, links): (i64, i64) = sql.query_row("SELECT (SELECT count(*) FROM job_runtime_job_record), (SELECT count(*) FROM job_runtime_job_link)", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    assert_eq!((records, links), (1, 1));
    Ok(())
}

#[test]
fn failed_history_transaction_preserves_settings_and_allows_later_writes() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("failure.db");
    let db = database(&path)?;
    let mut settings = db.settings();
    assert_eq!(commit(&mut settings, 0, 200_000)?, 1);
    let sql = Connection::open(&path)?;
    sql.execute_batch("CREATE TRIGGER reject_link BEFORE INSERT ON job_runtime_job_link BEGIN SELECT RAISE(ABORT, 'test'); END;")?;
    let history = db.history(|| 250);
    assert!(history.append(&summary()).is_err());
    let count: i64 = sql.query_row("SELECT count(*) FROM job_runtime_job_record", [], |r| {
        r.get(0)
    })?;
    assert_eq!(count, 0);
    assert_eq!(
        restore(db.settings())?
            .snapshot()
            .values
            .get(space::SHARED_TABLE_MAX_ENTRIES),
        Some(&json!(200_000))
    );
    assert_eq!(commit(&mut settings, 1, 300_000)?, 2);
    Ok(())
}

#[test]
fn checkpoint_waits_for_external_reader_and_reports_busy_before_retry() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("checkpoint.db");
    let db = database(&path)?;
    let mut settings = db.settings();
    commit(&mut settings, 0, 200_000)?;

    let mut reader =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    reader.busy_timeout(std::time::Duration::ZERO)?;
    let snapshot = reader.transaction()?;
    let _: i64 = snapshot.query_row("SELECT count(*) FROM user_env_setting", [], |row| {
        row.get(0)
    })?;
    commit(&mut settings, 1, 300_000)?;

    assert!(matches!(
        db.checkpoint_wal(),
        Err(SharedDatabaseCheckpointError::Wal(
            WalCheckpointError::Busy { .. }
        ))
    ));
    drop(snapshot);
    db.checkpoint_wal()?;
    assert_eq!(commit(&mut settings, 2, 400_000)?, 3);
    Ok(())
}

#[test]
fn close_refuses_another_shared_owner_and_succeeds_after_last_owner_is_released() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("close-owner.db");
    let database = database(&path)?;
    let mut settings = database.settings();
    commit(&mut settings, 0, 200_000)?;
    drop(settings);
    let other_owner = database.clone();

    let database = match database.close() {
        Err(SharedDatabaseCloseError::StillReferenced { database, owners }) => {
            assert_eq!(owners, 2);
            database
        }
        Err(error) => return Err(format!("unexpected close error: {error:?}").into()),
        Ok(_) => return Err("close succeeded while another shared owner remained".into()),
    };
    let mut settings = other_owner.settings();
    assert_eq!(commit(&mut settings, 1, 300_000)?, 2);
    drop(settings);
    drop(other_owner);

    database.close()?;
    let reopened = Connection::open(&path)?;
    let value: String = reopened.query_row(
        "SELECT value_json FROM user_env_setting WHERE setting_key = ?1",
        [du_user_env::space::SHARED_TABLE_MAX_ENTRIES],
        |row| row.get(0),
    )?;
    assert_eq!(serde_json::from_str::<i64>(&value)?, 300_000);
    Ok(())
}

#[test]
fn close_checkpoint_failure_retains_the_last_handle_for_a_safe_retry() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("close-checkpoint.db");
    let database = database(&path)?;
    let mut settings = database.settings();
    commit(&mut settings, 0, 200_000)?;

    let mut reader =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    reader.busy_timeout(std::time::Duration::ZERO)?;
    let snapshot = reader.transaction()?;
    let _: i64 = snapshot.query_row("SELECT count(*) FROM user_env_setting", [], |row| {
        row.get(0)
    })?;
    commit(&mut settings, 1, 300_000)?;
    drop(settings);

    let database = match database.close() {
        Err(SharedDatabaseCloseError::Checkpoint {
            database,
            error: WalCheckpointError::Busy { .. },
        }) => database,
        Err(error) => return Err(format!("unexpected close error: {error:?}").into()),
        Ok(_) => return Err("close succeeded while a reader pinned the WAL".into()),
    };
    drop(snapshot);
    database.close()?;
    Ok(())
}
