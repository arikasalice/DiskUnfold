use super::DEFINITION;
use rusqlite::{Connection, TransactionBehavior, params};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn production_migration_preserves_existing_settings_and_reopens() -> TestResult {
    use crate::{AppDb, HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION, migration};

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let backup = dir.path().join("backup.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    for step in &HISTORY_DEFINITIONS.steps[..2] {
        let before = dir.path().join(format!("before-{}.db", step.number));
        assert_eq!(
            migration::apply_next(&path, &before, &HISTORY_DEFINITIONS)
                .map_err(|e| format!("{e:?}"))?,
            step.number
        );
    }
    let db = Connection::open(&path)?;
    db.execute_batch("INSERT INTO user_env_setting VALUES ('scope.remember_recent', 'false', 'screen', 1, 2, 1); UPDATE user_env_setting_version SET settings_version=7;")?;
    drop(db);
    assert_eq!(
        migration::apply_next(&path, &backup, &HISTORY_DEFINITIONS)
            .map_err(|e| format!("{e:?}"))?,
        4
    );
    assert_eq!(
        migration::apply_next(
            &path,
            &dir.path().join("before-ruleset.db"),
            &HISTORY_DEFINITIONS
        )
        .map_err(|e| format!("{e:?}"))?,
        HISTORY_SCHEMA_VERSION
    );
    let db = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let result: (String, i64, i64) = db.connection.query_row(
        "SELECT (SELECT value_json FROM user_env_setting WHERE setting_key='scope.remember_recent'),
                settings_version,
                (SELECT count(*) FROM user_env_recent_scope)
         FROM user_env_setting_version",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(result, ("false".into(), 7, 0));
    drop(db);
    let old = AppDb::open_read_only(&backup).map_err(|e| format!("{e:?}"))?;
    assert_eq!(old.schema_version()?, 3);
    let value: String = old.connection.query_row(
        "SELECT value_json FROM user_env_setting WHERE setting_key='scope.remember_recent'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(value, "false");
    Ok(())
}

#[test]
fn synthetic_upsert_and_capacity_transaction_respects_unique_key() -> TestResult {
    let mut db = Connection::open_in_memory()?;
    db.execute_batch(DEFINITION)?;
    let write = "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,?3,1,?4,?4,?4,1) ON CONFLICT(scope_fingerprint,fingerprint_format) DO UPDATE SET last_used_at_ms=excluded.last_used_at_ms,updated_at_ms=excluded.updated_at_ms,row_version=row_version+1";
    db.execute(
        write,
        params![vec![1u8; 16], vec![1u8; 32], "{\"old\":true}", 1],
    )?;
    db.execute(
        write,
        params![vec![2u8; 16], vec![1u8; 32], "{\"new\":true}", 2],
    )?;
    let row: (String, i64, i64) = db.query_row(
        "SELECT scope_body,last_used_at_ms,row_version FROM user_env_recent_scope",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(row, ("{\"old\":true}".into(), 2, 2));
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(write, params![vec![3u8; 16], vec![3u8; 32], "{}", 3])?;
    tx.execute("DELETE FROM user_env_recent_scope WHERE recent_scope_id IN (SELECT recent_scope_id FROM user_env_recent_scope ORDER BY last_used_at_ms DESC LIMIT -1 OFFSET 1)", [])?;
    tx.commit()?;
    let last: i64 = db.query_row(
        "SELECT last_used_at_ms FROM user_env_recent_scope",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(last, 3);
    Ok(())
}

#[test]
fn schema_reopens_and_enforces_fingerprint_and_body_boundaries() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("recent.db");
    let db = Connection::open(&path)?;
    db.execute_batch(DEFINITION)?;
    let insert = "INSERT INTO user_env_recent_scope(recent_scope_id,scope_fingerprint,scope_body,include_count,last_used_at_ms,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,1,1,1,1)";
    db.execute(insert, params![vec![1u8; 16], vec![2u8; 32], "{}"])?;
    assert!(
        db.execute(insert, params![vec![3u8; 16], vec![2u8; 32], "{} "])
            .is_err()
    );
    assert!(
        db.execute(insert, params![vec![4u8; 16], vec![4u8; 32], "{"])
            .is_err()
    );
    assert!(
        db.execute(insert, params![vec![5u8; 15], vec![5u8; 32], "{} "])
            .is_err()
    );
    drop(db);
    let db = Connection::open(path)?;
    let row: (String, i64, i64) = db.query_row(
        "SELECT scope_body, fingerprint_format, row_version FROM user_env_recent_scope",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(row, ("{}".into(), 1, 1));
    Ok(())
}

#[test]
fn failed_eviction_rolls_back_insert_and_existing_rows() -> TestResult {
    let mut db = Connection::open_in_memory()?;
    db.execute_batch(DEFINITION)?;
    db.execute(
        "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,'{}',1,10,10,10,1)",
        params![vec![1u8; 16], vec![1u8; 32]],
    )?;
    db.execute_batch("CREATE TRIGGER reject_eviction BEFORE DELETE ON user_env_recent_scope BEGIN SELECT RAISE(ABORT, 'test'); END;")?;
    {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,'{}',1,20,20,20,1)",
            params![vec![2u8; 16], vec![2u8; 32]],
        )?;
        assert!(
            tx.execute(
                "DELETE FROM user_env_recent_scope WHERE last_used_at_ms=10",
                []
            )
            .is_err()
        );
    }
    let result: (i64, i64) = db.query_row(
        "SELECT count(*), max(last_used_at_ms) FROM user_env_recent_scope",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(result, (1, 10));
    Ok(())
}
