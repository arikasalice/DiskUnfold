use super::*;
use du_core_contracts::settings::{
    SETTINGS_SCHEMA_VERSION, SettingsChanges, SettingsCommit, SettingsSource, SettingsStore,
    SettingsStoreError,
};
use serde_json::json;
use std::collections::BTreeMap;
type StoredSettingRow = (String, String, String, i64, i64, i64);
const EMPTY_TRANSITIONS: &[(u32, u32)] = &[(1, 2), (2, 3)];

#[test]
fn read_only_legacy_settings_load_does_not_advance_schema() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("readonly.db");
    let db = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch(
        "UPDATE user_env_setting_version SET schema_version=1, settings_version=5;
        INSERT INTO user_env_setting VALUES ('future.key', '3', 'owner', 1, 2, 3);",
    )?;
    drop(db);
    let before = std::fs::read(&path)?;
    let mut reader = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
    let snapshot = reader.load().map_err(|e| format!("{e:?}"))?;
    assert_eq!((snapshot.schema_version, snapshot.revision), (1, 5));
    drop(reader);
    assert_eq!(std::fs::read(&path)?, before);
    Ok(())
}

#[test]
fn concurrent_empty_transitions_apply_once() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("race.db");
    let db = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.connection
        .execute_batch("UPDATE user_env_setting_version SET schema_version=1;")?;
    drop(db);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut db = AppDb::open_gui(&path).map_err(|e| format!("{e:?}"))?;
                barrier.wait();
                db.advance_empty_settings_schema(EMPTY_TRANSITIONS)
                    .map_err(|e| format!("{e:?}"))
            })
        })
        .collect();
    let mut results = Vec::new();
    for worker in workers {
        results.push(worker.join().map_err(|_| "thread failed")?);
    }
    results.sort();
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Ok(Some(1)))
            .count(),
        1
    );
    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|result| match result {
        Ok(None) => true,
        Err(error) => error == "Conflict",
        _ => false,
    }));
    Ok(())
}

#[test]
fn ignored_or_rewritten_schema_update_is_rolled_back() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui(&dir.path().join("ignored.db")).map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch(
        "UPDATE user_env_setting_version SET schema_version=1,settings_version=7;
        INSERT INTO user_env_setting VALUES ('future.key','3','owner',1,2,3);",
    )?;
    for trigger in [
        "CREATE TRIGGER prevent BEFORE UPDATE OF schema_version ON user_env_setting_version BEGIN SELECT RAISE(IGNORE); END;",
        "CREATE TRIGGER prevent AFTER UPDATE OF schema_version ON user_env_setting_version WHEN NEW.schema_version=3 BEGIN UPDATE user_env_setting_version SET schema_version=1 WHERE version_key='settings'; END;",
    ] {
        db.connection.execute_batch(trigger)?;
        assert_eq!(
            db.advance_empty_settings_schema(EMPTY_TRANSITIONS),
            Err(SettingsStoreError::Conflict)
        );
        let state: (i64, i64, i64) = db.connection.query_row("SELECT schema_version,settings_version,(SELECT count(*) FROM user_env_setting) FROM user_env_setting_version", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        assert_eq!(state, (1, 7, 1));
        db.connection.execute_batch("DROP TRIGGER prevent;")?;
    }
    Ok(())
}

#[test]
fn empty_schema_transition_preserves_rows_revision_and_rolls_back_on_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut db = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    let mut stale = AppDb::open_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch("UPDATE user_env_setting_version SET schema_version=1, settings_version=7, updated_at_ms=9;
        INSERT INTO user_env_setting VALUES ('future.key', '{\"x\": 1}', 'owner', 3, 4, 5);
        INSERT INTO user_env_setting VALUES ('locale.display_language', '\"ko\"', 'screen', 6, 7, 8);")?;
    let rows = || -> Result<Vec<StoredSettingRow>, rusqlite::Error> {
        let reader = rusqlite::Connection::open(&path)?;
        let mut q = reader.prepare("SELECT setting_key,value_json,modified_source,created_at_ms,updated_at_ms,row_version FROM user_env_setting ORDER BY setting_key")?;
        q.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect()
    };
    let before = rows()?;
    assert_eq!(
        db.advance_empty_settings_schema(&[(1, 2)]),
        Err(SettingsStoreError::Conflict),
        "an absent later stage is not an empty registered stage"
    );
    assert_eq!(
        db.advance_empty_settings_schema(&[(2, 3), (1, 2)]),
        Err(SettingsStoreError::Conflict),
        "the complete transition registry must be ordered and contiguous"
    );
    db.connection.execute_batch("CREATE TRIGGER reject_schema BEFORE UPDATE OF schema_version ON user_env_setting_version BEGIN SELECT RAISE(ABORT, 'injected'); END;")?;
    assert_eq!(
        db.advance_empty_settings_schema(EMPTY_TRANSITIONS),
        Err(SettingsStoreError::Storage)
    );
    assert_eq!(rows()?, before);
    let version: (i64, i64, i64) = db.connection.query_row(
        "SELECT settings_version,schema_version,updated_at_ms FROM user_env_setting_version",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(version, (7, 1, 9));
    db.connection.execute_batch(
        "DROP TRIGGER reject_schema;
        CREATE TABLE schema_update_audit(schema_version INTEGER NOT NULL);
        CREATE TRIGGER record_schema_update AFTER UPDATE OF schema_version ON user_env_setting_version
        BEGIN INSERT INTO schema_update_audit VALUES (NEW.schema_version); END;",
    )?;
    assert_eq!(
        db.advance_empty_settings_schema(EMPTY_TRANSITIONS),
        Ok(Some(1))
    );
    let updates: Vec<u32> = db
        .connection
        .prepare("SELECT schema_version FROM schema_update_audit ORDER BY rowid")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    assert_eq!(updates, [SETTINGS_SCHEMA_VERSION]);
    assert_eq!(
        stale.advance_empty_settings_schema(EMPTY_TRANSITIONS),
        Ok(None)
    );
    assert_eq!(rows()?, before);
    let version: (i64, i64, i64) = db.connection.query_row(
        "SELECT settings_version,schema_version,updated_at_ms FROM user_env_setting_version",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(version, (7, 3, 9));
    assert_eq!(
        db.advance_empty_settings_schema(&[(2, 3)]),
        Err(SettingsStoreError::Conflict)
    );
    Ok(())
}

#[test]
fn final_schema_update_failure_rolls_back_all_registered_empty_transitions()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("final-transition-failure.db");
    let mut db = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch(
        "UPDATE user_env_setting_version SET schema_version=1,settings_version=8,updated_at_ms=9;
        INSERT INTO user_env_setting VALUES ('scope.remember_recent','false','screen',3,7,4);
        CREATE TRIGGER reject_final_update BEFORE UPDATE OF schema_version ON user_env_setting_version
        WHEN NEW.schema_version=3 BEGIN SELECT RAISE(ABORT, 'injected final update failure'); END;",
    )?;
    let before: (i64, i64, i64, String, String, i64, i64, i64) = db.connection.query_row(
        "SELECT v.settings_version, v.schema_version, v.updated_at_ms, s.setting_key, s.value_json, s.created_at_ms, s.updated_at_ms, s.row_version FROM user_env_setting_version v CROSS JOIN user_env_setting s",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
    )?;
    assert_eq!(
        db.advance_empty_settings_schema(EMPTY_TRANSITIONS),
        Err(SettingsStoreError::Storage)
    );
    let after: (i64, i64, i64, String, String, i64, i64, i64) = db.connection.query_row(
        "SELECT v.settings_version, v.schema_version, v.updated_at_ms, s.setting_key, s.value_json, s.created_at_ms, s.updated_at_ms, s.row_version FROM user_env_setting_version v CROSS JOIN user_env_setting s",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
    )?;
    assert_eq!(after, before);
    assert_eq!(after.1, 1);
    Ok(())
}

#[test]
fn failed_transition_header_observation_reads_the_current_schema_after_a_race()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("failed-transition-race.db");
    let mut db = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch(
        "UPDATE user_env_setting_version SET schema_version=1,settings_version=7;
        CREATE TRIGGER reject_schema BEFORE UPDATE OF schema_version ON user_env_setting_version
        WHEN NEW.schema_version=3 BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    )?;
    assert_eq!(
        db.advance_empty_settings_schema(EMPTY_TRANSITIONS),
        Err(SettingsStoreError::Storage)
    );

    // A concurrent writer may change the header after rollback but before the
    // restore report observes it. The report must not claim the stale schema 1.
    let external = rusqlite::Connection::open(&path)?;
    external.execute(
        "UPDATE user_env_setting_version SET schema_version=2,settings_version=8 WHERE version_key='settings'",
        [],
    )?;
    assert_eq!(db.load_settings_schema_header_for_restore(), Ok((8, 2)));
    Ok(())
}

#[test]
fn failed_schema_header_read_returns_no_observation() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut db =
        AppDb::create_gui(&dir.path().join("missing-header.db")).map_err(|e| format!("{e:?}"))?;
    db.connection
        .execute_batch("DROP TABLE user_env_setting_version;")?;
    assert_eq!(
        db.load_settings_schema_header_for_restore(),
        Err(SettingsStoreError::Storage)
    );
    Ok(())
}

#[test]
fn commit_rejects_schema_or_revision_changes_without_touching_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut first = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    let mut other = AppDb::open_gui(&path).map_err(|e| format!("{e:?}"))?;
    let ctx = SettingsCommit {
        source: SettingsSource::Screen,
        timestamp_ms: 7,
    };
    let changes = SettingsChanges::from([("key".into(), Some(json!(3)))]);
    other
        .commit(0, &changes, ctx)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        first.commit(0, &changes, ctx),
        Err(SettingsStoreError::Conflict)
    );
    other
        .connection
        .execute_batch("UPDATE user_env_setting_version SET schema_version = 4;")?;
    assert_eq!(
        first.commit(1, &changes, ctx),
        Err(SettingsStoreError::Conflict)
    );
    let version: (i64, i64) = first.connection.query_row(
        "SELECT settings_version, schema_version FROM user_env_setting_version",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(version, (1, 4));
    let raw: String = first.connection.query_row(
        "SELECT value_json FROM user_env_setting WHERE setting_key = 'key'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(raw, "3");
    Ok(())
}

#[test]
fn database_open_and_settings_category_failures_are_separate()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    assert!(AppDb::open_read_only(&dir.path().join("missing.db")).is_err());
    let mut db = AppDb::create_gui(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    db.connection
        .execute_batch("DROP TABLE user_env_setting;")?;
    assert_eq!(
        db.load_settings_for_restore(),
        Err(SettingsStoreError::Storage)
    );
    db.connection
        .execute_batch("UPDATE user_env_setting_version SET schema_version = 4;")?;
    let (revision, schema, rows) = db
        .load_settings_for_restore()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!((revision, schema, rows.len()), (0, 4, 0));
    Ok(())
}

#[test]
fn quarantine_is_atomic_and_preserves_unselected_rows() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    let changes = SettingsChanges::from([
        ("bad".into(), Some(json!(1))),
        ("good".into(), Some(json!(2))),
        ("future".into(), Some(json!(3))),
    ]);
    db.commit(
        0,
        &changes,
        SettingsCommit {
            source: SettingsSource::Screen,
            timestamp_ms: 1,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let mismatch = BTreeMap::from([("bad".into(), "1".into()), ("good".into(), "99".into())]);
    assert_eq!(
        db.quarantine_settings(1, &mismatch, 2),
        Err(SettingsStoreError::Conflict)
    );
    assert_eq!(db.load().map_err(|e| format!("{e:?}"))?.values.len(), 3);
    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM user_env_setting_quarantine",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    assert_eq!(
        db.quarantine_settings(0, &BTreeMap::from([("bad".into(), "1".into())]), 2),
        Err(SettingsStoreError::Conflict)
    );
    assert_eq!(
        db.quarantine_settings(1, &BTreeMap::from([("bad".into(), "1".into())]), 2),
        Ok(2)
    );
    let snapshot = db.load().map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.values.get("good"), Some(&json!(2)));
    assert_eq!(snapshot.values.get("future"), Some(&json!(3)));
    assert!(!snapshot.values.contains_key("bad"));
    let entry: (String, String, String) = db.connection.query_row(
        "SELECT setting_key, raw_value_json, reason_key FROM user_env_setting_quarantine",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(
        entry,
        ("bad".into(), "1".into(), "settings.value_invalid".into())
    );
    let id: Vec<u8> = db.connection.query_row(
        "SELECT quarantine_id FROM user_env_setting_quarantine",
        [],
        |row| row.get(0),
    )?;
    let id = uuid::Uuid::from_slice(&id)?;
    assert_eq!(id.get_version_num(), 7);
    assert_eq!(id.get_variant(), uuid::Variant::RFC4122);
    Ok(())
}

#[test]
fn raw_restore_retains_malformed_rows_and_can_quarantine_them()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch("PRAGMA ignore_check_constraints=ON; INSERT INTO user_env_setting VALUES ('bad', '{bad', 'screen', 0, 0, 1); PRAGMA ignore_check_constraints=OFF;")?;
    assert_eq!(db.load(), Err(SettingsStoreError::Storage));
    let (revision, schema, rows) = db
        .load_settings_for_restore()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!((revision, schema), (0, SETTINGS_SCHEMA_VERSION));
    assert_eq!(rows.get("bad").map(String::as_str), Some("{bad"));
    assert_eq!(db.quarantine_settings(0, &rows, 5), Ok(1));
    let raw: String = db.connection.query_row(
        "SELECT raw_value_json FROM user_env_setting_quarantine",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(raw, "{bad");
    Ok(())
}

#[test]
fn quarantine_mid_batch_delete_failure_rolls_back_and_can_retry_after_reopen()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("quarantine-atomic.db");
    let mut db = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    let context = SettingsCommit {
        source: SettingsSource::Screen,
        timestamp_ms: 7,
    };
    let initial = SettingsChanges::from([
        ("a.invalid".into(), Some(json!(1))),
        ("b.invalid".into(), Some(json!(2))),
        ("z.valid".into(), Some(json!(3))),
    ]);
    assert_eq!(db.commit(0, &initial, context), Ok(1));
    db.connection.execute_batch("CREATE TRIGGER reject_second_delete BEFORE DELETE ON user_env_setting WHEN OLD.setting_key = 'b.invalid' BEGIN SELECT RAISE(ABORT, 'injected'); END;")?;
    let observed = BTreeMap::from([
        ("a.invalid".into(), "1".into()),
        ("b.invalid".into(), "2".into()),
    ]);
    assert_eq!(
        db.quarantine_settings(1, &observed, 10),
        Err(SettingsStoreError::Storage)
    );
    drop(db);
    let mut reopened = AppDb::open_gui(&path).map_err(|e| format!("{e:?}"))?;
    let (revision, schema, rows) = reopened
        .load_settings_for_restore()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!((revision, schema), (1, SETTINGS_SCHEMA_VERSION));
    assert_eq!(
        rows,
        BTreeMap::from([
            ("a.invalid".into(), "1".into()),
            ("b.invalid".into(), "2".into()),
            ("z.valid".into(), "3".into())
        ])
    );
    let count: i64 = reopened.connection.query_row(
        "SELECT count(*) FROM user_env_setting_quarantine",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 0);
    reopened
        .connection
        .execute_batch("DROP TRIGGER reject_second_delete;")?;
    assert_eq!(reopened.quarantine_settings(1, &observed, 11), Ok(2));
    assert_eq!(
        reopened.quarantine_settings(1, &observed, 12),
        Err(SettingsStoreError::Conflict)
    );
    drop(reopened);
    let mut final_reader = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
    let (revision, schema, rows) = final_reader
        .load_settings_for_restore()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!((revision, schema), (2, SETTINGS_SCHEMA_VERSION));
    assert_eq!(rows, BTreeMap::from([("z.valid".into(), "3".into())]));
    let mut stmt = final_reader.connection.prepare("SELECT setting_key, raw_value_json, quarantined_at_ms FROM user_env_setting_quarantine ORDER BY setting_key")?;
    let quarantined = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        quarantined,
        vec![
            ("a.invalid".into(), "1".into(), 11),
            ("b.invalid".into(), "2".into(), 11)
        ]
    );
    Ok(())
}
