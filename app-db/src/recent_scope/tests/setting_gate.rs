use super::*;
use du_core_contracts::settings::{
    SettingsChanges, SettingsCommit, SettingsSource, SettingsStore, SettingsStoreError,
};
use serde_json::json;
use std::time::Duration;

const REMEMBER_RECENT: &str = "scope.remember_recent";

#[test]
fn resolve_gate_reads_off_without_writing_a_recent_row() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let mut settings = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    settings
        .commit(
            0,
            &SettingsChanges::from([(REMEMBER_RECENT.into(), Some(json!(false)))]),
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 10,
            },
        )
        .map_err(|error| format!("{error:?}"))?;

    let mut store = RecentScopeStore::new(&mut db, NonZeroUsize::new(2).ok_or("zero")?, &valid)
        .map_err(|error| format!("{error:?}"))?;
    let enabled = store
        .resolve_with_setting(REMEMBER_RECENT, |value| value.unwrap_or(true))
        .map_err(|error| format!("{error:?}"))?;

    assert!(!enabled);
    assert!(
        store
            .list()
            .map_err(|error| format!("{error:?}"))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn resolve_gate_serializes_the_setting_read_with_setting_commits() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let mut settings = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    settings.connection.busy_timeout(Duration::ZERO)?;
    let off = SettingsChanges::from([(REMEMBER_RECENT.into(), Some(json!(false)))]);
    let context = SettingsCommit {
        source: SettingsSource::Screen,
        timestamp_ms: 20,
    };
    let mut store = RecentScopeStore::new(&mut db, NonZeroUsize::new(2).ok_or("zero")?, &valid)
        .map_err(|error| format!("{error:?}"))?;

    let enabled = store
        .resolve_with_setting(REMEMBER_RECENT, |value| {
            assert_eq!(value, None);
            assert_eq!(
                settings.commit(0, &off, context),
                Err(SettingsStoreError::Busy)
            );
            value.unwrap_or(true)
        })
        .map_err(|error| format!("{error:?}"))?;

    assert!(enabled);
    assert_eq!(settings.commit(0, &off, context), Ok(1));
    Ok(())
}

#[test]
fn resolve_gate_defaults_on_when_a_settings_table_is_known_missing() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("app.db"))
        .map_err(|error| format!("{error:?}"))?;
    db.connection.execute("DROP TABLE user_env_setting", [])?;
    let mut store = RecentScopeStore::new(&mut db, NonZeroUsize::new(2).ok_or("zero")?, &valid)
        .map_err(|error| format!("{error:?}"))?;

    let enabled = store
        .resolve_with_setting(REMEMBER_RECENT, |value| value.unwrap_or(true))
        .map_err(|error| format!("{error:?}"))?;

    assert!(enabled);
    Ok(())
}

#[test]
fn resolve_gate_propagates_other_sql_read_failures() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("app.db"))
        .map_err(|error| format!("{error:?}"))?;
    db.connection.execute_batch(
        "ALTER TABLE user_env_setting_version RENAME COLUMN schema_version TO broken_schema_version",
    )?;
    let mut store = RecentScopeStore::new(&mut db, NonZeroUsize::new(2).ok_or("zero")?, &valid)
        .map_err(|error| format!("{error:?}"))?;

    assert_eq!(
        store.resolve_with_setting(REMEMBER_RECENT, |value| value.unwrap_or(true)),
        Err(RecentError::Storage)
    );
    Ok(())
}

#[test]
fn checked_write_propagates_other_sql_read_failures_without_inserting() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("app.db"))
        .map_err(|error| format!("{error:?}"))?;
    db.connection.execute_batch(
        "ALTER TABLE user_env_setting_version RENAME COLUMN schema_version TO broken_schema_version",
    )?;
    let item = write(1, 1, 10);
    let mut store = RecentScopeStore::new(&mut db, NonZeroUsize::new(2).ok_or("zero")?, &valid)
        .map_err(|error| format!("{error:?}"))?;

    assert_eq!(
        store.put_with_setting_checked(&item, REMEMBER_RECENT, |value| value.unwrap_or(true)),
        Err(RecentError::Storage)
    );
    assert!(
        store
            .list()
            .map_err(|error| format!("{error:?}"))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn checked_write_recovers_from_a_known_missing_settings_table() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("app.db"))
        .map_err(|error| format!("{error:?}"))?;
    db.connection.execute("DROP TABLE user_env_setting", [])?;
    let item = write(1, 1, 10);
    let mut store = RecentScopeStore::new(&mut db, NonZeroUsize::new(2).ok_or("zero")?, &valid)
        .map_err(|error| format!("{error:?}"))?;

    assert!(
        store
            .put_with_setting_checked(&item, REMEMBER_RECENT, |value| value.unwrap_or(true))
            .map_err(|error| format!("{error:?}"))?
    );
    assert_eq!(store.list().map_err(|error| format!("{error:?}"))?.len(), 1);
    Ok(())
}

#[test]
fn resolve_gate_returns_busy_when_another_writer_holds_the_database() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    db.connection.busy_timeout(Duration::ZERO)?;
    let lock = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    lock.connection.execute_batch("BEGIN IMMEDIATE")?;

    let mut store = RecentScopeStore::new(&mut db, NonZeroUsize::new(2).ok_or("zero")?, &valid)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.resolve_with_setting(REMEMBER_RECENT, |value| value.unwrap_or(true)),
        Err(RecentError::Busy)
    );
    lock.connection.execute_batch("ROLLBACK")?;
    Ok(())
}
