//! Verify logical settings migration across desktop startup.
use super::*;

#[test]
fn gui_startup_transitions_legacy_settings_and_reports_migration()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("legacy.db");
    seed(&path, 200_000)?;
    let db = Connection::open(&path)?;
    db.execute_batch("UPDATE user_env_setting_version SET schema_version = 1;")?;
    drop(db);
    let service = open_settings(&path, 100)?;
    assert!(service.restore_status().migrated);
    assert_eq!(service.restore_status().stored_schema, Some(1));
    assert_eq!(service.snapshot().revision, 1);
    assert_eq!(service.snapshot().schema_version, 2);
    assert_eq!(
        service.snapshot().values.get("future.setting"),
        Some(&json!("preserved"))
    );
    assert_eq!(
        restore_shared_table_limit(service.snapshot()).map(|v| v.entries()),
        Some(200_000)
    );
    drop(service);
    let db = Connection::open(&path)?;
    let (schema, count): (i64, i64) = db.query_row("SELECT schema_version, (SELECT count(*) FROM user_env_setting) FROM user_env_setting_version", [], |r| Ok((r.get(0)?,r.get(1)?)))?;
    assert_eq!((schema, count), (2, 2));
    Ok(())
}

#[test]
fn current_schema_restores_while_another_connection_holds_write_lock()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("locked-current.db");
    seed(&path, 200_000)?;
    let database = test_database(&path)?;
    let lock = Connection::open(&path)?;
    lock.execute_batch("BEGIN IMMEDIATE;")?;
    let service = restore_settings(&database, 100, &["en", "ko"])?;
    assert!(!service.restore_status().write_locked);
    assert!(!service.restore_status().migrated);
    assert_eq!(
        restore_shared_table_limit(service.snapshot()).map(|v| v.entries()),
        Some(200_000)
    );
    lock.execute_batch("ROLLBACK;")?;
    lock.execute_batch("UPDATE user_env_setting_version SET schema_version=3; BEGIN IMMEDIATE;")?;
    let newer = restore_settings(&database, 100, &["en", "ko"])?;
    assert!(newer.restore_status().write_locked);
    assert_eq!(
        newer.restore_status().failure_reason,
        Some("settings.schema_newer")
    );
    lock.execute_batch("ROLLBACK;")?;
    Ok(())
}

#[test]
fn failed_logical_transition_starts_locked_on_defaults_without_touching_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("legacy-failure.db");
    seed(&path, 200_000)?;
    let db = Connection::open(&path)?;
    db.execute_batch("UPDATE user_env_setting_version SET schema_version=1;
        CREATE TRIGGER reject_transition BEFORE UPDATE OF schema_version ON user_env_setting_version BEGIN SELECT RAISE(ABORT, 'test'); END;")?;
    drop(db);
    let service = open_settings(&path, 100)?;
    assert!(service.restore_status().write_locked);
    assert_eq!(
        service.restore_status().failure_reason,
        Some("settings.restore_failed")
    );
    assert!(!service.restore_status().migrated);
    assert!(service.snapshot().values.is_empty());
    drop(service);
    let db = Connection::open(&path)?;
    let state: (i64, i64, i64) = db.query_row("SELECT schema_version,settings_version,(SELECT count(*) FROM user_env_setting) FROM user_env_setting_version", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    assert_eq!(state, (1, 1, 2));
    Ok(())
}
