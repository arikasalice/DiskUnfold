use super::*;
use rusqlite::hooks::{AuthAction, Authorization};

#[test]
fn missing_settings_table_defaults_to_remembered_limit_100_when_transaction_survives() -> TestResult
{
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("app.db"))
        .map_err(|error| format!("{error:?}"))?;
    let cap = NonZeroUsize::new(500).ok_or("zero cap")?;
    for index in 1..=100 {
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|error| format!("{error:?}"))?
            .put(&indexed_write(index, index as i64))
            .map_err(|error| format!("{error:?}"))?;
    }
    db.connection.execute("DROP TABLE user_env_setting", [])?;

    let mut store =
        RecentScopeStore::new(&mut db, cap, &valid).map_err(|error| format!("{error:?}"))?;
    assert!(
        store
            .put_with_settings(
                &indexed_write(101, 101),
                "scope.remember_recent",
                "scope.recent_limit",
                test_write_limit,
            )
            .map_err(|error| format!("{error:?}"))?
    );

    let rows = store.list().map_err(|error| format!("{error:?}"))?;
    assert_eq!(rows.len(), 100);
    assert_eq!(rows.first().map(|row| row.id), Some(indexed_id(101)));
    assert_eq!(rows.last().map(|row| row.id), Some(indexed_id(2)));
    assert!(
        store
            .get(&indexed_id(1))
            .map_err(|error| format!("{error:?}"))?
            .is_none()
    );
    assert!(
        store
            .get(&indexed_id(101))
            .map_err(|error| format!("{error:?}"))?
            .is_some()
    );
    Ok(())
}

#[test]
fn denied_settings_select_is_not_defaulted_and_rolls_back() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("app.db"))
        .map_err(|error| format!("{error:?}"))?;
    let cap = NonZeroUsize::new(500).ok_or("zero cap")?;
    RecentScopeStore::new(&mut db, cap, &valid)
        .map_err(|error| format!("{error:?}"))?
        .put(&write(1, 1, 10))
        .map_err(|error| format!("{error:?}"))?;
    db.connection.authorizer(Some(
        |context: rusqlite::hooks::AuthContext<'_>| match context.action {
            AuthAction::Read {
                table_name: "user_env_setting",
                ..
            } => Authorization::Deny,
            _ => Authorization::Allow,
        },
    ))?;

    let mut store =
        RecentScopeStore::new(&mut db, cap, &valid).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.put_with_settings(
            &write(2, 2, 20),
            "scope.remember_recent",
            "scope.recent_limit",
            test_write_limit,
        ),
        Err(RecentError::Storage)
    );
    assert_eq!(store.list().map_err(|error| format!("{error:?}"))?.len(), 1);
    assert_eq!(
        store
            .get(&id(1))
            .map_err(|error| format!("{error:?}"))?
            .map(|row| row.summary.last_used_at_ms),
        Some(10)
    );
    Ok(())
}

#[test]
fn begin_failure_is_not_defaulted_or_written() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let blocker = rusqlite::Connection::open(&path)?;
    db.connection.busy_timeout(std::time::Duration::ZERO)?;
    blocker.busy_timeout(std::time::Duration::ZERO)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    let cap = NonZeroUsize::new(500).ok_or("zero cap")?;

    assert_eq!(
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|error| format!("{error:?}"))?
            .put_with_settings(
                &write(1, 1, 10),
                "scope.remember_recent",
                "scope.recent_limit",
                test_write_limit,
            ),
        Err(RecentError::Busy)
    );
    blocker.execute_batch("ROLLBACK")?;
    assert!(
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|error| format!("{error:?}"))?
            .list()
            .map_err(|error| format!("{error:?}"))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn insert_failure_is_not_defaulted_and_rolls_back() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("app.db"))
        .map_err(|error| format!("{error:?}"))?;
    let cap = NonZeroUsize::new(500).ok_or("zero cap")?;
    db.connection.execute_batch(
        "CREATE TRIGGER reject_product_insert BEFORE INSERT ON user_env_recent_scope BEGIN SELECT RAISE(ABORT,'test'); END;",
    )?;

    assert_eq!(
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|error| format!("{error:?}"))?
            .put_with_settings(
                &write(1, 1, 10),
                "scope.remember_recent",
                "scope.recent_limit",
                test_write_limit,
            ),
        Err(RecentError::Storage)
    );
    assert!(
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|error| format!("{error:?}"))?
            .list()
            .map_err(|error| format!("{error:?}"))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn commit_failure_is_not_defaulted_and_rolls_back() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("app.db"))
        .map_err(|error| format!("{error:?}"))?;
    let cap = NonZeroUsize::new(500).ok_or("zero cap")?;
    db.connection.commit_hook(Some(|| true))?;

    assert_eq!(
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|error| format!("{error:?}"))?
            .put_with_settings(
                &write(1, 1, 10),
                "scope.remember_recent",
                "scope.recent_limit",
                test_write_limit,
            ),
        Err(RecentError::Storage)
    );
    db.connection.commit_hook(None::<fn() -> bool>)?;
    assert!(
        RecentScopeStore::new(&mut db, cap, &valid)
            .map_err(|error| format!("{error:?}"))?
            .list()
            .map_err(|error| format!("{error:?}"))?
            .is_empty()
    );
    Ok(())
}
