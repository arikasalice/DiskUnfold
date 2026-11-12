//! Verify logical settings migration across desktop startup.
use super::*;

type RawSettingRow = (String, String, String, i64, i64, i64);

fn raw_setting_rows(
    path: &std::path::Path,
) -> Result<Vec<RawSettingRow>, Box<dyn std::error::Error>> {
    let db = Connection::open(path)?;
    let mut statement = db.prepare("SELECT setting_key,value_json,modified_source,created_at_ms,updated_at_ms,row_version FROM user_env_setting ORDER BY setting_key")?;
    Ok(statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .collect::<Result<_, _>>()?)
}

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
    assert_eq!(service.snapshot().schema_version, SETTINGS_SCHEMA_VERSION);
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
    assert_eq!((schema, count), (i64::from(SETTINGS_SCHEMA_VERSION), 2));
    let recent_limit_rows: i64 = db.query_row(
        "SELECT count(*) FROM user_env_setting WHERE setting_key='scope.recent_limit'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(recent_limit_rows, 0);
    Ok(())
}

#[test]
fn gui_startup_transitions_schema_two_without_rewriting_rows_or_creating_defaults()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("schema-two.db");
    seed(&path, 200_000)?;
    let db = Connection::open(&path)?;
    db.execute_batch(
        "UPDATE user_env_setting_version SET schema_version=2,settings_version=17,updated_at_ms=31;
        INSERT INTO user_env_setting VALUES ('scope.remember_recent','false','owner',5,9,4);",
    )?;
    let physical_schema_before: i64 =
        db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    drop(db);
    let before = raw_setting_rows(&path)?;

    let database = test_database(&path)?;
    let settings = StartupSettings::open(&database, 100, &["en", "ko"])?;
    assert!(settings.status.migrated);
    assert_eq!(settings.status.stored_schema, Some(2));
    assert_eq!(settings.status.app_schema, SETTINGS_SCHEMA_VERSION);
    assert_eq!(settings.status.stored_revision, Some(17));
    {
        let service = settings.service.lock().map_err(|_| "settings poisoned")?;
        assert_eq!(service.snapshot().schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(service.snapshot().revision, 17);
        assert_eq!(
            service.snapshot().values.get("scope.remember_recent"),
            Some(&json!(false))
        );
        assert!(!service.snapshot().values.contains_key("scope.recent_limit"));
        let (_, catalog) = service.catalog();
        let recent_limit = catalog
            .iter()
            .find(|entry| entry.descriptor.key == "scope.recent_limit")
            .ok_or("missing recent limit descriptor")?;
        assert!(recent_limit.uses_default);
        assert_eq!(recent_limit.value, &json!(100));
    }
    drop(settings);
    drop(database);

    assert_eq!(raw_setting_rows(&path)?, before);
    let db = Connection::open(&path)?;
    let (revision, schema, updated_at): (i64, i64, i64) = db.query_row(
        "SELECT settings_version,schema_version,updated_at_ms FROM user_env_setting_version",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!((revision, schema, updated_at), (17, 3, 31));
    let default_rows: i64 = db.query_row(
        "SELECT count(*) FROM user_env_setting WHERE setting_key='scope.recent_limit'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(default_rows, 0);
    let physical_schema_after: i64 =
        db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(physical_schema_after, physical_schema_before);
    drop(db);

    let reopened = test_database(&path)?;
    let restarted = StartupSettings::open(&reopened, 101, &["en", "ko"])?;
    assert!(!restarted.status.migrated);
    assert_eq!(
        restarted.status.stored_schema,
        Some(SETTINGS_SCHEMA_VERSION)
    );
    assert_eq!(restarted.status.stored_revision, Some(17));
    drop(restarted);
    drop(reopened);
    assert_eq!(raw_setting_rows(&path)?, before);
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
    lock.execute(
        "UPDATE user_env_setting_version SET schema_version=?1",
        [SETTINGS_SCHEMA_VERSION + 1],
    )?;
    lock.execute_batch("BEGIN IMMEDIATE;")?;
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
        CREATE TRIGGER reject_transition BEFORE UPDATE OF schema_version ON user_env_setting_version WHEN NEW.schema_version=3 BEGIN SELECT RAISE(ABORT, 'test'); END;")?;
    drop(db);
    let rows_before = raw_setting_rows(&path)?;
    let db = Connection::open(&path)?;
    let header_before: (i64, i64, i64) = db.query_row(
        "SELECT settings_version,schema_version,updated_at_ms FROM user_env_setting_version WHERE version_key='settings'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let physical_schema_before: i64 =
        db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    drop(db);

    let database = test_database(&path)?;
    let settings = StartupSettings::open(&database, 100, &["en", "ko"])?;
    assert!(settings.status.write_locked);
    assert_eq!(settings.status.stored_schema, Some(1));
    assert_eq!(settings.status.stored_revision, Some(1));
    assert!(!settings.status.migrated);
    assert_eq!(
        settings.status.failure_reason,
        Some("settings.restore_failed")
    );
    assert!(
        settings
            .service
            .lock()
            .map_err(|_| "settings poisoned")?
            .snapshot()
            .values
            .is_empty()
    );

    // Exercise the registered GUI use case through the same IPC dispatcher as
    // the WebView bridge, including the actual serialized restoration report.
    let mut builder = EngineBuilder::default();
    settings.register_restore_status(&mut builder);
    let engine = builder.build()?;
    let request = json!({
        "command": "du_call",
        "args": {
            "envelope": {
                "name": "settings.restore_status",
                "contractVersion": 1,
                "correlationId": "0195f856-ab83-7a59-9343-10d29775c9a9",
                "body": {}
            }
        }
    });
    let response = crate::ipc::dispatch(&engine, &request.to_string()).ok_or("missing reply")?;
    let response: ResultEnvelope = serde_json::from_str(&response)?;
    assert!(response.error.is_none());
    assert_eq!(response.body["storedSchema"], json!(1));
    assert_eq!(response.body["appSchema"], json!(SETTINGS_SCHEMA_VERSION));
    assert_eq!(response.body["migrated"], json!(false));
    assert_eq!(response.body["writeLocked"], json!(true));
    let items = response.body["items"]
        .as_array()
        .ok_or("missing restore items")?;
    assert!(!items.is_empty());
    assert!(items.iter().all(|item| {
        item["source"] == json!("default") && item["reasonKey"] == json!("settings.restore_failed")
    }));
    drop(settings);
    drop(database);

    assert_eq!(raw_setting_rows(&path)?, rows_before);
    let db = Connection::open(&path)?;
    let header_after: (i64, i64, i64) = db.query_row(
        "SELECT settings_version,schema_version,updated_at_ms FROM user_env_setting_version WHERE version_key='settings'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(header_after, header_before);
    let physical_schema_after: i64 =
        db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(physical_schema_after, physical_schema_before);
    Ok(())
}

#[test]
fn failed_schema_header_observation_keeps_locked_defaults_and_wire_null()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("missing-schema-header.db");
    seed(&path, 200_000)?;
    let database = test_database(&path)?;
    let other = Connection::open(&path)?;
    other.execute_batch("DROP TABLE user_env_setting_version;")?;
    drop(other);

    let settings = StartupSettings::open(&database, 100, &["en", "ko"])?;
    assert!(settings.status.write_locked);
    assert_eq!(settings.status.stored_schema, None);
    assert!(!settings.status.migrated);
    assert!(
        settings
            .service
            .lock()
            .map_err(|_| "settings poisoned")?
            .snapshot()
            .values
            .is_empty()
    );

    let mut builder = EngineBuilder::default();
    settings.register_restore_status(&mut builder);
    let engine = builder.build()?;
    let request = json!({
        "command": "du_call",
        "args": {
            "envelope": {
                "name": "settings.restore_status",
                "contractVersion": 1,
                "correlationId": "0195f856-ab83-7a59-9343-10d29775c9a9",
                "body": {}
            }
        }
    });
    let response = crate::ipc::dispatch(&engine, &request.to_string()).ok_or("missing reply")?;
    let response: ResultEnvelope = serde_json::from_str(&response)?;
    assert!(response.error.is_none());
    assert_eq!(response.body["storedSchema"], serde_json::Value::Null);
    assert_eq!(response.body["migrated"], json!(false));
    assert_eq!(response.body["writeLocked"], json!(true));
    Ok(())
}
