use super::*;
use crate::{AppDb, HISTORY_DEFINITIONS};

fn seed(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let db = AppDb::create_gui(path).map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch("INSERT INTO user_env_setting VALUES ('locale.display_language', '\"ko\"', 'screen', 1, 2, 1); UPDATE user_env_setting_version SET settings_version=7;")?;
    Ok(())
}

fn state(path: &Path) -> Result<(u32, i64, String), Box<dyn std::error::Error>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let settings = db.query_row(
        "SELECT settings_version FROM user_env_setting_version",
        [],
        |r| r.get(0),
    )?;
    let value = db.query_row("SELECT value_json FROM user_env_setting", [], |r| r.get(0))?;
    Ok((version, settings, value))
}

#[test]
fn startup_definitions_preserve_settings_and_report_four_backup_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    seed(&path)?;
    for expected in [2, 3] {
        let outcome =
            apply_next_managed(&path, &root, &HISTORY_DEFINITIONS).map_err(|e| format!("{e:?}"))?;
        assert_eq!(outcome.revision, expected);
        assert!(outcome.retention.cleanup_unavailable);
    }
    assert_eq!(state(&path)?, (3, 7, "\"ko\"".into()));
    let first = root.join("backup-00000000000000000001").join(COPY);
    let second = root.join("backup-00000000000000000002").join(COPY);
    assert_eq!(state(&first)?, (1, 7, "\"ko\"".into()));
    assert_eq!(state(&second)?, (2, 7, "\"ko\"".into()));
    drop(AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?);
    let current = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert_eq!(
        publish(&current, 3, &root).map_err(|e| format!("{e:?}"))?,
        3
    );
    assert_eq!(
        inspect_retention(&root)
            .map_err(|e| format!("{e:?}"))?
            .content_valid,
        3
    );
    assert_eq!(
        publish(&current, 3, &root).map_err(|e| format!("{e:?}"))?,
        4
    );
    let report = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(report.content_valid, 4);
    assert!(report.cleanup_unavailable);
    for sequence in 1..=4 {
        assert!(root.join(format!("backup-{sequence:020}")).is_dir());
    }
    Ok(())
}

#[test]
fn failed_backup_and_interrupted_write_leave_the_startup_database_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    seed(&path)?;
    let missing = root.join("missing").join("backup.db");
    let result = apply_next_with_backup(
        &path,
        &HISTORY_DEFINITIONS,
        |source, _| backup_snapshot(source, &missing).map_err(backup_error),
        || Ok(()),
    );
    assert!(matches!(result, Err(MigrationError::Backup(_))));
    assert_eq!(state(&path)?, (1, 7, "\"ko\"".into()));
    assert!(!missing.exists());
    let result = apply_next_with_backup(
        &path,
        &HISTORY_DEFINITIONS,
        |source, version| publish(source, version, &root).map(|_| ()),
        || Err(MigrationError::Integrity),
    );
    assert!(matches!(result, Err(MigrationError::Integrity)));
    assert_eq!(state(&path)?, (1, 7, "\"ko\"".into()));
    assert!(valid_entry(&root.join("backup-00000000000000000001"), 1));
    assert_eq!(
        state(&root.join("backup-00000000000000000001").join(COPY))?,
        (1, 7, "\"ko\"".into())
    );
    Ok(())
}

#[test]
fn failed_history_definition_rolls_back_all_ddl_and_preserves_backup()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    seed(&path)?;
    let sql = format!(
        "{}\nINSERT INTO missing_table VALUES(1);",
        HISTORY_DEFINITIONS.steps[0].sql
    );
    let steps = [crate::migration::Definition {
        number: HISTORY_DEFINITIONS.steps[0].number,
        sql: &sql,
    }];
    let result = apply_next_managed(
        &path,
        &root,
        &Definitions {
            baseline: 1,
            steps: &steps,
        },
    );
    assert!(result.is_err());
    assert_eq!(state(&path)?, (1, 7, "\"ko\"".into()));
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert!(!db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='app_db_schema_meta')",
        [],
        |row| row.get::<_, bool>(0)
    )?);
    assert!(valid_entry(&root.join("backup-00000000000000000001"), 1));
    assert_eq!(
        state(&root.join("backup-00000000000000000001").join(COPY))?,
        (1, 7, "\"ko\"".into())
    );
    Ok(())
}

#[test]
fn noncanonical_sequence_and_zero_are_never_counted_as_owned()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    seed(&path)?;
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    publish(&db, 1, &root).map_err(|e| format!("{e:?}"))?;
    publish(&db, 1, &root).map_err(|e| format!("{e:?}"))?;
    let normal = root.join("backup-00000000000000000001");
    let noncanonical = root.join("backup-+0000000000000000001");
    fs::rename(&normal, &noncanonical)?;
    let second = root.join("backup-00000000000000000002");
    let marker = second.join(MARKER);
    let mut value: Value = serde_json::from_slice(&fs::read(&marker)?)?;
    value["sequence"] = json!(0);
    fs::write(&marker, serde_json::to_vec(&value)?)?;
    fs::rename(&second, root.join("backup-00000000000000000000"))?;
    let report = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (report.content_valid, report.recognized, report.ignored),
        (0, 0, 2)
    );
    assert!(noncanonical.exists());
    Ok(())
}

#[test]
fn oversized_marker_keeps_the_copy_unrecognized_and_intact()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    seed(&path)?;
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    publish(&db, 1, &root).map_err(|e| format!("{e:?}"))?;
    let copy = root.join("backup-00000000000000000001");
    let marker = copy.join(MARKER);
    let mut file = OpenOptions::new().append(true).open(&marker)?;
    file.write_all(&[b' '; MAX_MARKER_BYTES])?;
    drop(file);
    let report = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (report.content_valid, report.recognized, report.ignored),
        (0, 0, 1)
    );
    assert!(marker.exists());
    assert_eq!(state(&copy.join(COPY))?, (1, 7, "\"ko\"".into()));
    Ok(())
}

#[test]
fn simultaneous_startup_snapshots_preserve_both_backups_and_apply_once()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::mpsc;
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    seed(&path)?;
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let path_one = path.clone();
    let root_one = root.clone();
    let first = std::thread::spawn(move || {
        apply_next_with_backup(
            &path_one,
            &HISTORY_DEFINITIONS,
            |source, version| publish(source, version, &root_one).map(|_| ()),
            || {
                ready_tx.send(()).map_err(|_| MigrationError::Integrity)?;
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .map_err(|_| MigrationError::Integrity)?;
                Ok(())
            },
        )
    });
    ready_rx.recv_timeout(std::time::Duration::from_secs(10))?;
    let path_two = path.clone();
    let root_two = root.clone();
    let second = std::thread::spawn(move || {
        apply_next_with_backup(
            &path_two,
            &HISTORY_DEFINITIONS,
            |source, version| publish(source, version, &root_two).map(|_| ()),
            || {
                release_tx.send(()).map_err(|_| MigrationError::Integrity)?;
                Ok(())
            },
        )
    });
    let results = [
        first.join().map_err(|_| "first thread panicked")?,
        second.join().map_err(|_| "second thread panicked")?,
    ];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(state(&path)?, (2, 7, "\"ko\"".into()));
    assert!(valid_entry(&root.join("backup-00000000000000000001"), 1));
    assert!(valid_entry(&root.join("backup-00000000000000000002"), 2));
    for sequence in 1..=2 {
        assert_eq!(
            state(&root.join(format!("backup-{sequence:020}")).join(COPY))?,
            (1, 7, "\"ko\"".into())
        );
    }
    Ok(())
}
