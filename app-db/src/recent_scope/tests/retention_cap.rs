use super::*;

fn test_write_limit(remember: Option<&str>, limit: Option<&str>) -> Option<NonZeroUsize> {
    if !remember
        .and_then(|value| serde_json::from_str::<bool>(value).ok())
        .unwrap_or(true)
    {
        return None;
    }
    let rows = limit
        .and_then(|value| serde_json::from_str::<usize>(value).ok())
        .filter(|value| [50, 100, 200, 500].contains(value))
        .unwrap_or(100);
    NonZeroUsize::new(rows)
}

fn indexed_id(index: usize) -> [u8; 16] {
    let mut value = [0; 16];
    value[6] = 0x70;
    value[8] = 0x80;
    value[14..].copy_from_slice(&(index as u16).to_be_bytes());
    value
}

fn indexed_write(index: usize, time: i64) -> RecentWrite<'static> {
    let mut fingerprint = [0; 32];
    fingerprint[..8].copy_from_slice(&(index as u64).to_be_bytes());
    RecentWrite {
        id: indexed_id(index),
        fingerprint,
        body: r#"{"approved":true}"#,
        include_count: 1,
        now_ms: time,
    }
}

fn commit_scope_settings(
    db: &mut AppDb,
    revision: u64,
    remember: bool,
    limit: usize,
    timestamp_ms: i64,
) -> Result<u64, du_core_contracts::settings::SettingsStoreError> {
    use du_core_contracts::settings::{
        SettingsChanges, SettingsCommit, SettingsSource, SettingsStore,
    };

    db.commit(
        revision,
        &SettingsChanges::from([
            (
                "scope.remember_recent".into(),
                Some(serde_json::json!(remember)),
            ),
            ("scope.recent_limit".into(), Some(serde_json::json!(limit))),
        ]),
        SettingsCommit {
            source: SettingsSource::Screen,
            timestamp_ms,
        },
    )
}

#[test]
fn persisted_cap_changes_apply_only_to_the_next_successful_write() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    commit_scope_settings(&mut db, 0, true, 500, 1).map_err(|e| format!("{e:?}"))?;
    let max_cap = NonZeroUsize::new(500).ok_or("zero cap")?;
    for index in 1..=60 {
        let item = indexed_write(index, index as i64);
        assert!(
            RecentScopeStore::new(&mut db, max_cap, &valid)
                .map_err(|e| format!("{e:?}"))?
                .put_with_settings(
                    &item,
                    "scope.remember_recent",
                    "scope.recent_limit",
                    test_write_limit,
                )
                .map_err(|e| format!("{e:?}"))?
        );
    }

    commit_scope_settings(&mut db, 1, true, 50, 2).map_err(|e| format!("{e:?}"))?;
    let listed = list_recent_scopes(&db, max_cap).map_err(|e| format!("{e:?}"))?;
    assert_eq!(listed.len(), 60);
    assert_eq!(listed.first().map(|row| row.id), Some(indexed_id(60)));
    assert!(
        read_recent_scope(&db, &indexed_id(1))
            .map_err(|e| format!("{e:?}"))?
            .is_some()
    );

    let next = indexed_write(61, 61);
    assert!(
        RecentScopeStore::new(&mut db, max_cap, &valid)
            .map_err(|e| format!("{e:?}"))?
            .put_with_settings(
                &next,
                "scope.remember_recent",
                "scope.recent_limit",
                test_write_limit,
            )
            .map_err(|e| format!("{e:?}"))?
    );
    let listed = list_recent_scopes(&db, max_cap).map_err(|e| format!("{e:?}"))?;
    assert_eq!(listed.len(), 50);
    assert_eq!(listed.first().map(|row| row.id), Some(indexed_id(61)));
    assert_eq!(listed.last().map(|row| row.id), Some(indexed_id(12)));
    assert!(
        read_recent_scope(&db, &indexed_id(1))
            .map_err(|e| format!("{e:?}"))?
            .is_none()
    );
    Ok(())
}

#[test]
fn product_settings_write_uses_one_immediate_transaction_and_fails_on_sql_errors() -> TestResult {
    use du_core_contracts::settings::{
        SettingsChanges, SettingsCommit, SettingsSource, SettingsStore, SettingsStoreError,
    };
    use serde_json::json;

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let mut settings = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    settings
        .connection
        .busy_timeout(std::time::Duration::ZERO)?;
    let context = SettingsCommit {
        source: SettingsSource::Screen,
        timestamp_ms: 30,
    };
    let update = SettingsChanges::from([
        ("scope.remember_recent".into(), Some(json!(false))),
        ("scope.recent_limit".into(), Some(json!(50))),
    ]);
    let cap = NonZeroUsize::new(500).ok_or("zero cap")?;
    {
        let mut store =
            RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
        assert!(
            store
                .put_with_settings(
                    &write(1, 1, 10),
                    "scope.remember_recent",
                    "scope.recent_limit",
                    |remember, limit| {
                        assert_eq!(remember, None);
                        assert_eq!(limit, None);
                        assert_eq!(
                            settings.commit(0, &update, context),
                            Err(SettingsStoreError::Busy)
                        );
                        Some(NonZeroUsize::new(500).unwrap_or(NonZeroUsize::MIN))
                    },
                )
                .map_err(|e| format!("{e:?}"))?
        );
    }
    assert_eq!(settings.commit(0, &update, context), Ok(1));
    {
        let mut store =
            RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
        assert!(
            !store
                .put_with_settings(
                    &write(2, 2, 20),
                    "scope.remember_recent",
                    "scope.recent_limit",
                    |remember, limit| {
                        assert_eq!(remember, Some("false"));
                        assert_eq!(limit, Some("50"));
                        test_write_limit(remember, limit)
                    },
                )
                .map_err(|e| format!("{e:?}"))?
        );
        assert_eq!(store.list().map_err(|e| format!("{e:?}"))?.len(), 1);
    }

    db.connection.execute(
        "UPDATE user_env_setting SET value_json='true' WHERE setting_key='scope.remember_recent'",
        [],
    )?;
    db.connection.execute(
        "UPDATE user_env_setting SET value_json=?1 WHERE setting_key='scope.recent_limit'",
        [serde_json::json!("51").to_string()],
    )?;
    {
        let mut store =
            RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
        assert!(
            store
                .put_with_settings(
                    &write(4, 4, 40),
                    "scope.remember_recent",
                    "scope.recent_limit",
                    |remember, limit| {
                        assert_eq!(remember, Some("true"));
                        assert_eq!(limit, Some("\"51\""));
                        test_write_limit(remember, limit)
                    },
                )
                .map_err(|e| format!("{e:?}"))?
        );
        assert_eq!(store.list().map_err(|e| format!("{e:?}"))?.len(), 2);
    }

    db.connection.execute("DROP TABLE user_env_setting", [])?;
    let mut store = RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        store.put_with_settings(
            &write(3, 3, 30),
            "scope.remember_recent",
            "scope.recent_limit",
            test_write_limit,
        ),
        Err(RecentError::Storage)
    );
    assert_eq!(store.list().map_err(|e| format!("{e:?}"))?.len(), 2);
    Ok(())
}

#[test]
fn product_setting_eviction_failure_rolls_back_the_candidate() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db =
        AppDb::create_gui_with_history(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    let cap = NonZeroUsize::new(500).ok_or("zero cap")?;
    RecentScopeStore::new(&mut db, cap, &valid)
        .map_err(|e| format!("{e:?}"))?
        .put(&write(1, 1, 10))
        .map_err(|e| format!("{e:?}"))?;
    db.connection.execute_batch(
        "CREATE TRIGGER reject_product_eviction BEFORE DELETE ON user_env_recent_scope BEGIN SELECT RAISE(ABORT,'test'); END;",
    )?;
    let mut store = RecentScopeStore::new(&mut db, cap, &valid).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        store.put_with_settings(
            &write(2, 2, 20),
            "scope.remember_recent",
            "scope.recent_limit",
            |_, _| NonZeroUsize::new(1),
        ),
        Err(RecentError::Storage)
    );
    assert_eq!(store.list().map_err(|e| format!("{e:?}"))?.len(), 1);
    assert_eq!(
        store
            .get(&id(1))
            .map_err(|e| format!("{e:?}"))?
            .ok_or("original row missing")?
            .summary
            .last_used_at_ms,
        10
    );
    Ok(())
}

#[test]
fn summary_reader_accepts_the_physical_limit_and_rejects_one_extra_row() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db =
        AppDb::create_gui_with_history(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    let physical_limit = NonZeroUsize::new(500).ok_or("zero cap")?;
    for index in 1..=500 {
        let item = indexed_write(index, index as i64);
        db.connection.execute(
            "INSERT INTO user_env_recent_scope(
                 recent_scope_id,scope_fingerprint,fingerprint_format,scope_body,include_count,
                 last_used_at_ms,created_at_ms,updated_at_ms,row_version
             ) VALUES(?1,?2,1,?3,1,?4,?4,?4,1)",
            params![
                item.id.as_slice(),
                item.fingerprint.as_slice(),
                item.body,
                item.now_ms
            ],
        )?;
    }
    assert_eq!(
        list_recent_scopes(&db, physical_limit)
            .map_err(|e| format!("{e:?}"))?
            .len(),
        500
    );
    let item = indexed_write(501, 501);
    db.connection.execute(
        "INSERT INTO user_env_recent_scope(
             recent_scope_id,scope_fingerprint,fingerprint_format,scope_body,include_count,
             last_used_at_ms,created_at_ms,updated_at_ms,row_version
         ) VALUES(?1,?2,1,?3,1,?4,?4,?4,1)",
        params![
            item.id.as_slice(),
            item.fingerprint.as_slice(),
            item.body,
            item.now_ms
        ],
    )?;
    assert_eq!(
        list_recent_scopes(&db, physical_limit),
        Err(RecentError::Corrupt)
    );
    Ok(())
}
