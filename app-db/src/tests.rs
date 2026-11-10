use super::*;
use du_core_contracts::settings::{
    SettingsChanges, SettingsCommit, SettingsSource, SettingsStore, SettingsStoreError,
};
use serde_json::json;

fn context() -> SettingsCommit {
    SettingsCommit {
        source: SettingsSource::Screen,
        timestamp_ms: 100,
    }
}

#[test]
fn desktop_bootstrap_creates_once_and_reopens_existing_overrides()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app-data").join("diskunfold.db");
    let mut first = AppDb::open_or_create_gui(&path).map_err(|e| format!("{e:?}"))?;
    let changes = SettingsChanges::from([(
        "space.shared_table_max_entries".into(),
        Some(json!(100_000)),
    )]);
    first
        .commit(0, &changes, context())
        .map_err(|e| format!("{e:?}"))?;
    drop(first);
    let mut next = AppDb::open_or_create_gui(&path).map_err(|e| format!("{e:?}"))?;
    let snapshot = next.load().map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.revision, 1);
    assert_eq!(
        snapshot.values.get("space.shared_table_max_entries"),
        Some(&json!(100_000))
    );
    Ok(())
}

#[test]
fn legacy_connection_keeps_required_pragmas() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("legacy.db");
    let db = AppDb::open(&path).map_err(|e| format!("{e:?}"))?;
    let values: (String, u32, u32, u32) = db.connection.query_row(
        "SELECT (SELECT journal_mode FROM pragma_journal_mode), (SELECT foreign_keys FROM pragma_foreign_keys), (SELECT timeout FROM pragma_busy_timeout), (SELECT synchronous FROM pragma_synchronous)", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(values, ("wal".into(), 1, 5000, 1));
    Ok(())
}

#[test]
fn legacy_open_and_read_only_do_not_migrate() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("legacy.db");
    let old = AppDb::open(&path).map_err(|e| format!("{e:?}"))?;
    assert_eq!(old.schema_version()?, 0);
    drop(old);
    let before = std::fs::read(&path)?;
    assert!(matches!(
        AppDb::open_read_only(&path),
        Err(OpenError::UnsupportedSchema(0))
    ));
    assert!(matches!(
        AppDb::open_gui(&path),
        Err(OpenError::MigrationRequired)
    ));
    assert_eq!(std::fs::read(&path)?, before);
    let missing = dir.path().join("missing.db");
    assert!(AppDb::open_read_only(&missing).is_err());
    assert!(!missing.exists());
    assert!(AppDb::create_gui(&path).is_err());
    Ok(())
}

#[test]
fn creation_failure_can_be_retried_without_removing_competing_file()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.db");
    let result = create_gui_with(
        &path,
        |tx| tx.execute_batch("INVALID SQL").map_err(OpenError::from),
        || Ok(()),
        AppDb::open_gui,
    );
    assert!(matches!(result, Err(OpenError::Sqlite(_))));
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
    let result = create_gui_with(
        &path,
        initialize_settings,
        || Err(OpenError::WalUnavailable),
        AppDb::open_gui,
    );
    assert!(matches!(result, Err(OpenError::WalUnavailable)));
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let before = std::fs::read(&path)?;
    assert!(AppDb::create_gui(&path).is_err());
    assert_eq!(std::fs::read(&path)?, before);
    Ok(())
}

#[test]
fn published_database_open_failure_reports_safe_retry() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.db");
    let result = create_gui_with(
        &path,
        initialize_settings,
        || Ok(()),
        |_| Err(OpenError::WalUnavailable),
    );
    assert!(matches!(result, Err(OpenError::CreatedButOpenFailed(_))));
    let mut db = AppDb::open_gui(&path).map_err(|e| format!("{e:?}"))?;
    assert_eq!(db.load().map_err(|e| format!("{e:?}"))?.revision, 0);
    Ok(())
}

#[test]
fn simultaneous_creation_never_removes_winner() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("race.db");
    let gate = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                create_gui_with(
                    &path,
                    initialize_settings,
                    || {
                        gate.wait();
                        Ok(())
                    },
                    AppDb::open_gui,
                )
                .is_ok()
            })
        })
        .collect();
    let mut winners = 0;
    for handle in handles {
        if handle.join().map_err(|_| "creator panicked")? {
            winners += 1;
        }
    }
    assert_eq!(winners, 1);
    assert!(AppDb::open_gui(&path).is_ok());
    Ok(())
}

#[test]
fn new_store_commits_atomically_and_reopens_read_only() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.db");
    let mut first = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    let mut second = AppDb::open_gui(&path).map_err(|e| format!("{e:?}"))?;
    let initial = first.load().map_err(|e| format!("{e:?}"))?;
    assert_eq!((initial.revision, initial.schema_version), (0, 1));
    let changes = SettingsChanges::from([
        ("one".into(), Some(json!(1))),
        ("two".into(), Some(json!(true))),
    ]);
    assert_eq!(first.commit(0, &changes, context()), Ok(1));
    let mut reader = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
    let live = reader.load().map_err(|e| format!("{e:?}"))?;
    assert_eq!(live.revision, 1);
    assert_eq!(live.values.get("two"), Some(&json!(true)));
    drop(reader);
    let metadata: (String, i64, i64, i64) = first.connection.query_row(
        "SELECT modified_source, created_at_ms, updated_at_ms, row_version FROM user_env_setting WHERE setting_key = 'one'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(metadata, ("screen".into(), 100, 100, 1));
    assert_eq!(
        second.commit(0, &changes, context()),
        Err(SettingsStoreError::Conflict)
    );
    first.connection.execute_batch("CREATE TRIGGER fail_settings BEFORE INSERT ON user_env_setting WHEN NEW.setting_key = 'z' BEGIN SELECT RAISE(ABORT, 'failure'); END;")?;
    let failed =
        SettingsChanges::from([("one".into(), Some(json!(2))), ("z".into(), Some(json!(3)))]);
    assert_eq!(
        first.commit(1, &failed, context()),
        Err(SettingsStoreError::Storage)
    );
    drop(first);
    drop(second);
    let before_read = std::fs::read(&path)?;
    let mut read = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
    let snapshot = read.load().map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.values.get("one"), Some(&json!(1)));
    assert_eq!(snapshot.values.get("two"), Some(&json!(true)));
    assert_eq!(
        read.commit(1, &changes, context()),
        Err(SettingsStoreError::Storage)
    );
    drop(read);
    assert_eq!(std::fs::read(&path)?, before_read);
    let mut write = AppDb::open_gui(&path).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        write.commit(1, &SettingsChanges::from([("one".into(), None)]), context()),
        Ok(2)
    );
    assert!(
        !write
            .load()
            .map_err(|e| format!("{e:?}"))?
            .values
            .contains_key("one")
    );
    Ok(())
}

#[test]
fn simultaneous_writers_only_commit_one_revision() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("concurrent.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|value| {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut db = AppDb::open_gui(&path).map_err(|e| format!("{e:?}"))?;
                barrier.wait();
                Ok::<_, String>(db.commit(
                    0,
                    &SettingsChanges::from([("value".into(), Some(json!(value)))]),
                    context(),
                ))
            })
        })
        .collect();
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.join().map_err(|_| "writer panicked")??);
    }
    assert_eq!(results.iter().filter(|r| **r == Ok(1)).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(SettingsStoreError::Conflict))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn foreign_and_newer_files_are_rejected_without_mutation() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let foreign = dir.path().join("foreign.db");
    let db = Connection::open(&foreign)?;
    db.execute("CREATE TABLE other (id INTEGER PRIMARY KEY)", [])?;
    drop(db);
    assert!(matches!(
        AppDb::open(&foreign),
        Err(OpenError::ForeignDatabase)
    ));
    assert!(matches!(
        AppDb::open_read_only(&foreign),
        Err(OpenError::ForeignDatabase)
    ));
    let newer = dir.path().join("newer.db");
    let db = Connection::open(&newer)?;
    db.pragma_update(None, "application_id", APPLICATION_ID)?;
    db.pragma_update(None, "user_version", HISTORY_SCHEMA_VERSION + 1)?;
    drop(db);
    let before = std::fs::read(&newer)?;
    assert!(matches!(
        AppDb::open_gui(&newer),
        Err(OpenError::UnsupportedSchema(4))
    ));
    assert!(matches!(
        AppDb::open_read_only(&newer),
        Err(OpenError::UnsupportedSchema(4))
    ));
    assert_eq!(std::fs::read(newer)?, before);
    Ok(())
}
