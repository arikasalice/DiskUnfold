use super::*;
use du_app_db::AppDb;
use du_core_contracts::settings::{SettingsChanges, SettingsCommit, SettingsSource, SettingsStore};
use du_user_env::settings::SettingsError;
use rusqlite::Connection;
use serde_json::json;
use std::path::Path;

fn test_database(path: &Path) -> Result<SharedDatabase, io::Error> {
    let database = AppDb::open_or_create_gui(path)
        .map_err(|error| io::Error::other(format!("db.open_failed: {error:?}")))?;
    Ok(SharedDatabase::new(database, None))
}

fn open_settings(
    path: &Path,
    timestamp_ms: i64,
) -> Result<SettingsService<SharedSettings>, io::Error> {
    restore_settings(&test_database(path)?, timestamp_ms, &["en", "ko"])
}

fn context() -> SettingsCommit {
    SettingsCommit {
        source: SettingsSource::Screen,
        timestamp_ms: 12,
    }
}

#[test]
fn failed_recent_setting_commit_leaves_existing_override_intact()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("failure.db");
    let mut db = AppDb::open_or_create_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.commit(
        0,
        &SettingsChanges::from([(scope::REMEMBER_RECENT.into(), Some(json!(false)))]),
        context(),
    )
    .map_err(|e| format!("{e:?}"))?;
    drop(db);
    let db = Connection::open(&path)?;
    db.execute_batch("CREATE TRIGGER reject_recent_delete BEFORE DELETE ON user_env_setting BEGIN SELECT RAISE(ABORT, 'test'); END;")?;
    drop(db);
    let mut service = open_settings(&path, 100)?;
    let reset = SettingsChanges::from([(scope::REMEMBER_RECENT.into(), None)]);
    let fingerprint = service.review(1, &reset).map_err(|e| format!("{e:?}"))?;
    assert!(service.apply(1, &reset, fingerprint, context()).is_err());
    assert!(!scope::remember_recent(service.snapshot()));
    drop(service);
    assert!(!scope::remember_recent(
        open_settings(&path, 101)?.snapshot()
    ));
    Ok(())
}

#[test]
fn startup_preserves_a_configured_language_override() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("language.db");
    let mut db = AppDb::open_or_create_gui(&path).map_err(|error| format!("{error:?}"))?;
    db.commit(
        0,
        &SettingsChanges::from([(locale::DISPLAY_LANGUAGE_KEY.into(), Some(json!("ko")))]),
        context(),
    )
    .map_err(|error| format!("{error:?}"))?;
    drop(db);

    let settings = StartupSettings::open(&test_database(&path)?, 100, &["en", "ko"])?;
    assert_eq!(settings.stored_language()?.as_deref(), Some("ko"));
    assert!(settings.status.invalid_keys.is_empty());
    assert_eq!(settings.status.stored_revision, Some(1));
    Ok(())
}

#[test]
fn recent_scope_setting_is_registered_and_persists_without_erasing_other_settings()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.db");
    seed(&path, 200_000)?;
    let mut service = open_settings(&path, 100)?;
    assert!(scope::remember_recent(service.snapshot()));
    let off = SettingsChanges::from([(scope::REMEMBER_RECENT.into(), Some(json!(false)))]);
    let fingerprint = service.review(1, &off).map_err(|e| format!("{e:?}"))?;
    service
        .apply(1, &off, fingerprint, context())
        .map_err(|e| format!("{e:?}"))?;
    assert!(!scope::remember_recent(service.snapshot()));
    drop(service);
    let mut service = open_settings(&path, 101)?;
    assert!(!scope::remember_recent(service.snapshot()));
    assert_eq!(
        service.snapshot().values.get("future.setting"),
        Some(&json!("preserved"))
    );
    let reset = SettingsChanges::from([(scope::REMEMBER_RECENT.into(), None)]);
    let fingerprint = service.review(2, &reset).map_err(|e| format!("{e:?}"))?;
    service
        .apply(2, &reset, fingerprint, context())
        .map_err(|e| format!("{e:?}"))?;
    assert!(scope::remember_recent(service.snapshot()));
    drop(service);
    assert!(scope::remember_recent(
        open_settings(&path, 102)?.snapshot()
    ));
    Ok(())
}

#[test]
fn recent_scope_setting_defaults_on_when_restoration_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.db");
    seed(&path, 200_000)?;
    let db = Connection::open(&path)?;
    db.execute_batch("UPDATE user_env_setting_version SET schema_version = 2;")?;
    drop(db);
    let service = open_settings(&path, 100)?;
    assert!(service.restore_status().write_locked);
    assert!(scope::remember_recent(service.snapshot()));
    Ok(())
}

#[test]
fn source_keeps_startup_service_and_report_alive() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("lifetime.db");
    seed(&path, 1)?;
    let settings = Arc::new(StartupSettings::open(
        &test_database(&path)?,
        100,
        &["en", "ko"],
    )?);
    let weak = Arc::downgrade(&settings);
    let source = settings.clone().scan_limit_source();
    drop(settings);
    let retained = weak.upgrade().ok_or("settings dropped")?;
    assert_eq!(retained.status.stored_schema, Some(1));
    assert_eq!(
        retained.status.items[space::SHARED_TABLE_MAX_ENTRIES].reason,
        Some("settings.value_invalid")
    );
    assert_eq!(source().entries(), 1_000_000);
    drop(retained);
    drop(source);
    assert!(weak.upgrade().is_none());
    Ok(())
}

#[test]
fn startup_quarantine_failure_keeps_all_rows_and_disables_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("failure.db");
    seed(&path, 1)?;
    let db = Connection::open(&path)?;
    db.execute_batch("CREATE TRIGGER reject_quarantine BEFORE INSERT ON user_env_setting_quarantine BEGIN SELECT RAISE(ABORT, 'test'); END;")?;
    drop(db);
    let mut service = open_settings(&path, 100)?;
    assert_eq!(
        service.restore_status().failure_reason,
        Some("settings.restore_failed")
    );
    assert!(service.restore_status().write_locked);
    assert_eq!(
        service.apply(0, &SettingsChanges::new(), [0; 32], context()),
        Err(SettingsError::RestoreLocked)
    );
    drop(service);
    let db = Connection::open(&path)?;
    let (values, quarantine, revision): (i64, i64, i64) = db.query_row("SELECT (SELECT count(*) FROM user_env_setting), (SELECT count(*) FROM user_env_setting_quarantine), settings_version FROM user_env_setting_version", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    assert_eq!((values, quarantine, revision), (2, 0, 1));
    Ok(())
}

fn seed(path: &Path, value: i64) -> Result<(), Box<dyn std::error::Error>> {
    let mut db = AppDb::open_or_create_gui(path).map_err(|e| format!("{e:?}"))?;
    db.commit(
        0,
        &SettingsChanges::from([
            (space::SHARED_TABLE_MAX_ENTRIES.into(), Some(json!(value))),
            ("future.setting".into(), Some(json!("preserved"))),
        ]),
        context(),
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[test]
fn startup_restores_valid_or_quarantines_only_invalid_registered_value()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let valid = dir.path().join("valid.db");
    seed(&valid, 200_000)?;
    let service = open_settings(&valid, 100)?;
    assert_eq!(
        restore_shared_table_limit(service.snapshot()).map(|v| v.entries()),
        Some(200_000)
    );
    assert!(service.restore_status().invalid_keys.is_empty());
    assert_eq!(
        service.snapshot().values.get("future.setting"),
        Some(&json!("preserved"))
    );
    drop(service);
    let invalid = dir.path().join("invalid.db");
    seed(&invalid, 1)?;
    let service = open_settings(&invalid, 123)?;
    assert_eq!(
        restore_shared_table_limit(service.snapshot()).map(|v| v.entries()),
        Some(1_000_000)
    );
    assert_eq!(
        service.restore_status().invalid_keys,
        vec![space::SHARED_TABLE_MAX_ENTRIES]
    );
    assert_eq!(
        service.snapshot().values.get("future.setting"),
        Some(&json!("preserved"))
    );
    drop(service);
    let db = Connection::open(&invalid)?;
    let row: (String, String, i64) = db.query_row(
        "SELECT setting_key, raw_value_json, quarantined_at_ms FROM user_env_setting_quarantine",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(
        row,
        (space::SHARED_TABLE_MAX_ENTRIES.into(), "1".into(), 123)
    );
    let count: i64 = db.query_row(
        "SELECT count(*) FROM user_env_setting WHERE setting_key = 'future.setting'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn startup_locks_newer_schema_and_category_failures_but_not_database_open_failures()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("newer.db");
    seed(&path, 200_000)?;
    let db = Connection::open(&path)?;
    db.execute_batch("UPDATE user_env_setting_version SET schema_version = 2;")?;
    drop(db);
    let mut service = open_settings(&path, 100)?;
    assert_eq!(
        service.restore_status().failure_reason,
        Some("settings.schema_newer")
    );
    assert!(service.restore_status().write_locked);
    assert_eq!(
        restore_shared_table_limit(service.snapshot()).map(|v| v.entries()),
        Some(1_000_000)
    );
    assert_eq!(
        service.review(0, &SettingsChanges::new()),
        Err(SettingsError::RestoreLocked)
    );
    assert_eq!(
        service.apply(0, &SettingsChanges::new(), [0; 32], context()),
        Err(SettingsError::RestoreLocked)
    );
    drop(service);
    let db = Connection::open(&path)?;
    let (schema, count): (i64, i64) = db.query_row("SELECT schema_version, (SELECT count(*) FROM user_env_setting) FROM user_env_setting_version", [], |r| Ok((r.get(0)?,r.get(1)?)))?;
    assert_eq!((schema, count), (2, 2));
    db.execute_batch(
        "UPDATE user_env_setting_version SET schema_version = 1; DROP TABLE user_env_setting;",
    )?;
    drop(db);
    let mut service = open_settings(&path, 100)?;
    assert_eq!(
        service.restore_status().failure_reason,
        Some("settings.restore_failed")
    );
    assert!(service.restore_status().write_locked);
    assert_eq!(
        service.apply(0, &SettingsChanges::new(), [0; 32], context()),
        Err(SettingsError::RestoreLocked)
    );
    let foreign = dir.path().join("foreign.db");
    Connection::open(&foreign)?.execute_batch("CREATE TABLE other(value TEXT);")?;
    assert!(open_settings(&foreign, 100).is_err());
    Ok(())
}
