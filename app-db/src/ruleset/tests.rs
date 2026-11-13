use super::{RulesetError, RulesetRow};
use crate::{AppDb, HISTORY_DEFINITIONS, migration};
use du_core_contracts::ruleset::{RulesetRead, RulesetReadError};
use std::sync::{Arc, Barrier};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn row(version: i64) -> RulesetRow {
    RulesetRow {
        version,
        ruleset_hash: vec![1; 32],
        hash_format: "du.protect.ruleset.v1".into(),
        class_table_version: "1".into(),
        known_location_table_version: "1".into(),
        rule_format_version: 1,
        origin: "product_update".into(),
        change_summary: "{}".into(),
        body: "{\"rules\":[]}".into(),
        created_at_ms: 1,
    }
}

#[test]
fn read_port_pins_owned_raw_row_and_distinguishes_failures() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("port.sqlite");
    let mut writer = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let reader = AppDb::open_read_only_live(&path).map_err(|e| format!("{e:?}"))?;
    let port: &dyn RulesetRead = &reader;
    assert_eq!(port.read_current(), Ok(None));
    writer
        .append_ruleset(0, &row(1))
        .map_err(|e| format!("{e:?}"))?;
    let pinned = port
        .read_version(1)
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing")?;
    let mut next = row(2);
    next.body = "{\"rules\":[1]}".into();
    writer
        .append_ruleset(1, &next)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(pinned, row(1));
    assert_eq!(port.read_version(1), Ok(Some(row(1))));
    assert_eq!(port.read_current(), Ok(Some(next)));
    assert_eq!(port.read_version(3), Ok(None));
    writer.connection.execute_batch("DROP TRIGGER change_planner_ruleset_no_update; PRAGMA ignore_check_constraints=ON; UPDATE change_planner_ruleset SET body='broken' WHERE version=2;")?;
    assert_eq!(port.read_current(), Err(RulesetReadError::Corrupt));
    assert_eq!(port.read_version(2), Err(RulesetReadError::Corrupt));
    assert_eq!(port.read_version(1), Ok(Some(row(1))));
    writer
        .connection
        .execute_batch("DROP TABLE change_planner_ruleset;")?;
    assert_eq!(
        port.read_current(),
        Err(RulesetReadError::StorageUnavailable)
    );
    assert_eq!(
        port.read_version(1),
        Err(RulesetReadError::StorageUnavailable)
    );
    Ok(())
}
#[test]
fn two_connections_race_and_previous_version_remains_readable() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db.sqlite");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    assert!(db.latest_ruleset().map_err(|e| format!("{e:?}"))?.is_none());
    db.append_ruleset(0, &row(1))
        .map_err(|e| format!("{e:?}"))?;
    drop(db);
    let first = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let second = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = [first, second]
        .into_iter()
        .map(|mut db| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                db.append_ruleset(1, &row(2))
            })
        })
        .collect();
    let mut successes = 0;
    let mut stale = 0;
    for thread in threads {
        let result = thread.join().map_err(|_| "thread failed")?;
        match result {
            Ok(()) => successes += 1,
            Err(RulesetError::Stale) => stale += 1,
            Err(e) => return Err(format!("{e:?}").into()),
        }
    }
    assert_eq!((successes, stale), (1, 1));
    let db = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        db.latest_ruleset().map_err(|e| format!("{e:?}"))?,
        Some(row(2))
    );
    assert_eq!(
        db.ruleset_version(1).map_err(|e| format!("{e:?}"))?,
        Some(row(1))
    );
    assert!(
        db.ruleset_version(3)
            .map_err(|e| format!("{e:?}"))?
            .is_none()
    );
    Ok(())
}
#[test]
fn insert_failure_rolls_back_and_append_only_triggers_hold() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db.sqlite");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    db.append_ruleset(0, &row(1))
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        db.connection
            .execute("UPDATE change_planner_ruleset SET body='{}'", [])
            .is_err()
    );
    assert!(
        db.connection
            .execute("DELETE FROM change_planner_ruleset", [])
            .is_err()
    );
    assert!(db.connection.execute("INSERT OR REPLACE INTO change_planner_ruleset SELECT * FROM change_planner_ruleset WHERE version=1",[]).is_err());
    db.connection.execute_batch("CREATE TRIGGER test_fail BEFORE INSERT ON change_planner_ruleset WHEN NEW.version=2 BEGIN SELECT RAISE(ABORT, 'injected'); END;")?;
    assert!(matches!(
        db.append_ruleset(1, &row(2)),
        Err(RulesetError::Sqlite(_))
    ));
    assert_eq!(
        db.latest_ruleset().map_err(|e| format!("{e:?}"))?,
        Some(row(1))
    );
    assert!(matches!(
        db.append_ruleset(0, &row(2)),
        Err(RulesetError::Stale)
    ));
    Ok(())
}
#[test]
fn corrupt_stored_json_is_not_returned_as_a_row() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db.sqlite");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    db.append_ruleset(0, &row(1))
        .map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch("DROP TRIGGER change_planner_ruleset_no_update; PRAGMA ignore_check_constraints=ON; UPDATE change_planner_ruleset SET body='broken';")?;
    assert!(matches!(db.latest_ruleset(), Err(RulesetError::Corrupt)));
    assert!(matches!(db.ruleset_version(1), Err(RulesetError::Corrupt)));
    Ok(())
}
#[test]
fn version_four_data_survives_migration_five() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db.sqlite");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    for version in 2..=4 {
        migration::apply_next(
            &path,
            &dir.path().join(format!("{version}.bak")),
            &HISTORY_DEFINITIONS,
        )
        .map_err(|e| format!("{e:?}"))?;
    }
    let db = rusqlite::Connection::open(&path)?;
    db.execute(
        "INSERT INTO user_env_setting VALUES ('example', 'true', 'screen', 1, 2, 1)",
        [],
    )?;
    drop(db);
    assert_eq!(
        migration::apply_next(&path, &dir.path().join("5.bak"), &HISTORY_DEFINITIONS)
            .map_err(|e| format!("{e:?}"))?,
        5
    );
    let db = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let saved: String = db.connection.query_row(
        "SELECT value_json FROM user_env_setting WHERE setting_key='example'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(saved, "true");
    assert!(db.latest_ruleset().map_err(|e| format!("{e:?}"))?.is_none());
    Ok(())
}

#[test]
fn modified_ruleset_migration_digest_is_rejected_on_reopen() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db.sqlite");
    let db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch("DROP TRIGGER app_db_schema_meta_no_update; UPDATE app_db_schema_meta SET definition_hash=zeroblob(32) WHERE migration_no=5;")?;
    drop(db);
    assert!(
        matches!(AppDb::open_gui_with_history(&path),Err(crate::OpenError::HistorySchema(error)) if matches!(*error, migration::MigrationError::Tampered))
    );
    Ok(())
}

#[test]
fn structurally_valid_row_is_raw_and_does_not_assert_hash_integrity() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db.sqlite");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let supplied = row(1);
    db.append_ruleset(0, &supplied)
        .map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch("DROP TRIGGER change_planner_ruleset_no_update; UPDATE change_planner_ruleset SET body='{}';")?;
    let raw = db.latest_ruleset().map_err(|e| format!("{e:?}"))?;
    assert_eq!(raw.map(|value| value.body), Some("{}".into()));
    Ok(())
}
