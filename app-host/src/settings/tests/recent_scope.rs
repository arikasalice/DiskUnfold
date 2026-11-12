use super::*;
use du_user_env::scope;

#[test]
fn failed_recent_setting_commit_leaves_existing_override_intact()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("failure.db");
    let mut db = AppDb::open_or_create_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.commit(
        0,
        &SettingsChanges::from([(scope::REMEMBER_RECENT.into(), Some(json!(false)))]),
        context(),
    )
    .map_err(|e| format!("{e:?}"))?;
    drop(db);
    let db = Connection::open(&path)?;
    db.execute_batch("CREATE TRIGGER reject_recent_delete BEFORE DELETE ON user_env_setting BEGIN SELECT RAISE(ABORT, 'test'); END;")?;
    drop(db);
    let mut service = open_settings(&path, 100)?;
    let reset = SettingsChanges::from([(scope::REMEMBER_RECENT.into(), None)]);
    let fingerprint = service.review(1, &reset).map_err(|e| format!("{e:?}"))?;
    assert!(service.apply(1, &reset, fingerprint, context()).is_err());
    assert!(!scope::remember_recent(service.snapshot()));
    drop(service);
    assert!(!scope::remember_recent(
        open_settings(&path, 101)?.snapshot()
    ));
    Ok(())
}

#[test]
fn recent_scope_setting_is_registered_and_persists_without_erasing_other_settings()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.db");
    let mut database = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    database
        .commit(
            0,
            &SettingsChanges::from([
                (space::SHARED_TABLE_MAX_ENTRIES.into(), Some(json!(200_000))),
                ("future.setting".into(), Some(json!("preserved"))),
            ]),
            context(),
        )
        .map_err(|e| format!("{e:?}"))?;
    drop(database);
    let reopen =
        |timestamp| -> Result<SettingsService<SharedSettings>, Box<dyn std::error::Error>> {
            let db = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
            Ok(restore_settings(
                &SharedDatabase::new(db, None),
                timestamp,
                &["en", "ko"],
            )?)
        };
    let db = Connection::open(&path)?;
    db.execute(
        "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,?3,1,10,10,10,1)",
        rusqlite::params![vec![1_u8; 16], vec![2_u8; 32], "{}"],
    )?;
    drop(db);
    let recent_rows = || -> rusqlite::Result<Vec<Vec<rusqlite::types::Value>>> {
        let db = Connection::open(&path)?;
        let mut query =
            db.prepare("SELECT * FROM user_env_recent_scope ORDER BY recent_scope_id")?;
        let rows = query.query_map([], |row| (0..9).map(|index| row.get(index)).collect())?;
        rows.collect()
    };
    let original_rows = recent_rows()?;
    assert_eq!(original_rows.len(), 1);
    let mut service = reopen(100)?;
    assert!(scope::remember_recent(service.snapshot()));
    let off = SettingsChanges::from([(scope::REMEMBER_RECENT.into(), Some(json!(false)))]);
    let fingerprint = service.review(1, &off).map_err(|e| format!("{e:?}"))?;
    service
        .apply(1, &off, fingerprint, context())
        .map_err(|e| format!("{e:?}"))?;
    assert!(!scope::remember_recent(service.snapshot()));
    assert_eq!(recent_rows()?, original_rows);
    drop(service);
    let mut service = reopen(101)?;
    assert!(!scope::remember_recent(service.snapshot()));
    assert_eq!(recent_rows()?, original_rows);
    assert_eq!(
        service.snapshot().values.get("future.setting"),
        Some(&json!("preserved"))
    );
    let reset = SettingsChanges::from([(scope::REMEMBER_RECENT.into(), None)]);
    let fingerprint = service.review(2, &reset).map_err(|e| format!("{e:?}"))?;
    service
        .apply(2, &reset, fingerprint, context())
        .map_err(|e| format!("{e:?}"))?;
    assert!(scope::remember_recent(service.snapshot()));
    drop(service);
    assert!(scope::remember_recent(reopen(102)?.snapshot()));
    assert_eq!(recent_rows()?, original_rows);
    Ok(())
}

#[test]
fn recent_scope_setting_defaults_on_when_restoration_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.db");
    seed(&path, 200_000)?;
    let db = Connection::open(&path)?;
    db.execute_batch("UPDATE user_env_setting_version SET schema_version = 3;")?;
    drop(db);
    let service = open_settings(&path, 100)?;
    assert!(service.restore_status().write_locked);
    assert!(scope::remember_recent(service.snapshot()));
    Ok(())
}

#[test]
fn shared_database_reads_limit_per_write_and_list_keeps_old_rows_until_then()
-> Result<(), Box<dyn std::error::Error>> {
    use du_app_db::recent_scope::RecentWrite;
    use std::{num::NonZeroUsize, sync::atomic::AtomicBool};

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("recent-limit.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    db.commit(
        0,
        &SettingsChanges::from([
            (scope::REMEMBER_RECENT.into(), Some(json!(true))),
            (scope::RECENT_LIMIT.into(), Some(json!(500))),
        ]),
        context(),
    )
    .map_err(|e| format!("{e:?}"))?;
    let database = SharedDatabase::new(db, None);
    let cap = NonZeroUsize::new(2).ok_or("zero compatibility cap")?;
    let physical_limit = NonZeroUsize::new(scope::RECENT_LIMIT_MAX).ok_or("zero list limit")?;
    let stopped = AtomicBool::new(false);
    let make_write = |index: usize, time: i64| {
        let mut id = [0; 16];
        id[6] = 0x70;
        id[8] = 0x80;
        id[14..].copy_from_slice(&(index as u16).to_be_bytes());
        let mut fingerprint = [0; 32];
        fingerprint[..8].copy_from_slice(&(index as u64).to_be_bytes());
        RecentWrite {
            id,
            fingerprint,
            body: r#"{"fixture":true}"#,
            include_count: 1,
            now_ms: time,
        }
    };
    for index in 1..=60 {
        database
            .put_recent_scope(cap, &make_write(index, index as i64), &stopped)
            .map_err(|e| format!("{e:?}"))?;
    }
    let mut settings = database.settings();
    settings
        .commit(
            1,
            &SettingsChanges::from([(scope::RECENT_LIMIT.into(), Some(json!(50)))]),
            context(),
        )
        .map_err(|e| format!("{e:?}"))?;
    let existing = database
        .recent_scope_summaries(physical_limit)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(existing.len(), 60);
    assert_eq!(existing.first().map(|row| row.last_used_at_ms), Some(60));

    database
        .put_recent_scope(cap, &make_write(61, 61), &stopped)
        .map_err(|e| format!("{e:?}"))?;
    let retained = database
        .recent_scope_summaries(physical_limit)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(retained.len(), 50);
    assert_eq!(retained.first().map(|row| row.last_used_at_ms), Some(61));
    assert_eq!(retained.last().map(|row| row.last_used_at_ms), Some(12));

    settings
        .commit(
            2,
            &SettingsChanges::from([(scope::REMEMBER_RECENT.into(), Some(json!(false)))]),
            context(),
        )
        .map_err(|e| format!("{e:?}"))?;
    database
        .put_recent_scope(cap, &make_write(62, 62), &stopped)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        database
            .recent_scope_summaries(physical_limit)
            .map_err(|e| format!("{e:?}"))?
            .len(),
        50
    );
    Ok(())
}
