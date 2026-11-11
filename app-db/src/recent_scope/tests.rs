use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn full_database_rejects_write_without_changing_existing_row() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db =
        AppDb::create_gui_with_history(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    let cap = NonZeroUsize::new(2).ok_or("zero")?;
    RecentScopeStore::new(&mut db, cap, &valid)
        .map_err(|e| format!("{e:?}"))?
        .put(&write(1, 1, 10))
        .map_err(|e| format!("{e:?}"))?;
    let pages: i64 = db
        .connection
        .pragma_query_value(None, "page_count", |row| row.get(0))?;
    db.connection.pragma_update(None, "max_page_count", pages)?;
    let body = format!(
        r#"{{"approved":true,"payload":"{}"}}"#,
        "x".repeat(2_000_000)
    );
    let item = RecentWrite {
        id: id(2),
        fingerprint: [2; 32],
        body: &body,
        include_count: 1,
        now_ms: 20,
    };
    assert_eq!(
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|e| format!("{e:?}"))?
            .put(&item),
        Err(RecentError::DiskFull)
    );
    let store = RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
    let rows = store.list().map_err(|e| format!("{e:?}"))?;
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].id, rows[0].last_used_at_ms), (id(1), 10));
    Ok(())
}

fn id(n: u8) -> [u8; 16] {
    let mut value = [0; 16];
    value[6] = 0x70;
    value[8] = 0x80;
    value[15] = n;
    value
}

fn write(n: u8, fingerprint: u8, time: i64) -> RecentWrite<'static> {
    RecentWrite {
        id: id(n),
        fingerprint: [fingerprint; 32],
        body: r#"{"approved":true}"#,
        include_count: 1,
        now_ms: time,
    }
}

fn valid(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .is_ok_and(|value| value.get("approved") == Some(&serde_json::Value::Bool(true)))
}

#[test]
fn summaries_deduplicate_evict_and_reopen() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let cap = NonZeroUsize::new(2).ok_or("zero")?;
    let mut store = RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
    store.put(&write(1, 1, 10)).map_err(|e| format!("{e:?}"))?;
    store.put(&write(2, 2, 20)).map_err(|e| format!("{e:?}"))?;
    let mut duplicate = write(3, 1, 20);
    duplicate.body = r#"{"approved":true,"changed":true}"#;
    store.put(&duplicate).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        store
            .list()
            .map_err(|e| format!("{e:?}"))?
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        vec![id(1), id(2)]
    );
    store.put(&write(4, 4, 30)).map_err(|e| format!("{e:?}"))?;
    drop(db);
    let mut db = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let store = RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        store
            .list()
            .map_err(|e| format!("{e:?}"))?
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        vec![id(4), id(1)]
    );
    assert!(store.get(&id(2)).map_err(|e| format!("{e:?}"))?.is_none());
    assert_eq!(
        store
            .get(&id(1))
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing")?
            .body,
        write(1, 1, 10).body
    );
    Ok(())
}

/// A delayed observation cannot demote a more recently used scope before eviction.
#[test]
fn delayed_duplicate_preserves_recent_order_across_connections() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut first = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let mut second = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let cap = NonZeroUsize::new(2).ok_or("zero")?;
    let mut newest =
        RecentScopeStore::new(&mut first, cap, &valid).map_err(|e| format!("{e:?}"))?;
    let mut delayed =
        RecentScopeStore::new(&mut second, cap, &valid).map_err(|e| format!("{e:?}"))?;
    newest.put(&write(1, 1, 30)).map_err(|e| format!("{e:?}"))?;
    newest.put(&write(2, 2, 20)).map_err(|e| format!("{e:?}"))?;
    delayed
        .put(&write(3, 1, 10))
        .map_err(|e| format!("{e:?}"))?;
    newest.put(&write(4, 4, 40)).map_err(|e| format!("{e:?}"))?;
    let retained = delayed.list().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        retained
            .iter()
            .map(|entry| (entry.id, entry.last_used_at_ms))
            .collect::<Vec<_>>(),
        vec![(id(4), 40), (id(1), 30)]
    );
    Ok(())
}

#[test]
fn summary_corruption_and_unvalidated_body_never_become_partial_success() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db =
        AppDb::create_gui_with_history(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    db.connection.execute(
        "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,'{}',1,1,1,1,1)",
        params![id(1), [1_u8; 32]],
    )?;
    let cap = NonZeroUsize::new(1).ok_or("zero")?;
    let store = RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
    assert_eq!(store.list().map_err(|e| format!("{e:?}"))?.len(), 1);
    assert_eq!(store.get(&id(1)), Err(RecentError::Corrupt));
    db.connection.execute(
        "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,'{}',1,2,2,2,1)",
        params![id(2), [2_u8; 32]],
    )?;
    let store = RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
    assert_eq!(store.list(), Err(RecentError::Corrupt));
    db.connection.execute(
        "DELETE FROM user_env_recent_scope WHERE recent_scope_id=?1",
        [id(2)],
    )?;
    db.connection.execute(
        "UPDATE user_env_recent_scope SET recent_scope_id=?1",
        [[0_u8; 16]],
    )?;
    let store = RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
    assert_eq!(store.list(), Err(RecentError::Corrupt));
    db.connection
        .execute_batch("DROP TABLE user_env_recent_scope")?;
    let store = RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
    assert_eq!(store.list(), Err(RecentError::Storage));
    Ok(())
}

#[test]
fn failed_eviction_rolls_back_insert_and_existing_update() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db =
        AppDb::create_gui_with_history(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    let cap = NonZeroUsize::new(1).ok_or("zero")?;
    RecentScopeStore::new(&mut db, cap, &valid)
        .map_err(|e| format!("{e:?}"))?
        .put(&write(1, 1, 10))
        .map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch("CREATE TRIGGER reject_recent_delete BEFORE DELETE ON user_env_recent_scope BEGIN SELECT RAISE(ABORT,'test'); END;")?;
    assert_eq!(
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|e| format!("{e:?}"))?
            .put(&write(2, 2, 20)),
        Err(RecentError::Storage)
    );
    let original: (i64, i64) = db.connection.query_row(
        "SELECT last_used_at_ms,row_version FROM user_env_recent_scope WHERE recent_scope_id=?1",
        [id(1)],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(original, (10, 1));
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM user_env_recent_scope", [], |r| r
                .get::<_, i64>(0))?,
        1
    );
    // A pre-existing over-cap row makes an update and eviction share a failing transaction.
    db.connection.execute(
        "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,'{}',1,2,2,2,1)",
        params![id(3), [3_u8; 32]],
    )?;
    assert_eq!(
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|e| format!("{e:?}"))?
            .put(&write(4, 1, 30)),
        Err(RecentError::Storage)
    );
    let after: (i64, i64) = db.connection.query_row(
        "SELECT last_used_at_ms,row_version FROM user_env_recent_scope WHERE recent_scope_id=?1",
        [id(1)],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(after, original);
    Ok(())
}
