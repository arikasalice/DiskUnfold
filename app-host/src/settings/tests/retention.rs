use super::*;
use du_app_db::AppDb;
use du_core_contracts::settings::{SettingsChanges, SettingsStore};
use du_user_env::history_retention::{DAYS_KEY, ENABLED_KEY};
use serde_json::{Value, json};

#[test]
fn stored_policy_survives_reopening_the_sqlite_settings_store()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("history-retention.db");
    let mut database = AppDb::open_or_create_gui(&path).map_err(|error| format!("{error:?}"))?;
    database
        .commit(
            0,
            &SettingsChanges::from([
                (ENABLED_KEY.into(), Some(json!(true))),
                (DAYS_KEY.into(), Some(json!(180))),
            ]),
            context(),
        )
        .map_err(|error| format!("{error:?}"))?;
    drop(database);

    for _ in 0..2 {
        let settings = StartupSettings::open(&test_database(&path)?, 100, &["en", "ko"])?;
        let policy = settings.history_retention_policy();
        assert!(policy.enabled);
        assert_eq!(policy.specified_days, Some(180));
        assert_eq!(policy.suggested_days, 90);
        assert_eq!(policy.unavailable_reason, None);
    }
    Ok(())
}

#[test]
fn invalid_policy_rows_are_quarantined_and_never_enable_retention()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("history-retention-invalid.db");
    let mut database = AppDb::open_or_create_gui(&path).map_err(|error| format!("{error:?}"))?;
    database
        .commit(
            0,
            &SettingsChanges::from([
                (ENABLED_KEY.into(), Some(json!(true))),
                (DAYS_KEY.into(), Some(Value::Null)),
            ]),
            context(),
        )
        .map_err(|error| format!("{error:?}"))?;
    drop(database);

    let settings = StartupSettings::open(&test_database(&path)?, 100, &["en", "ko"])?;
    let policy = settings.history_retention_policy();
    assert!(!policy.enabled);
    assert_eq!(policy.specified_days, None);
    assert_eq!(
        policy.unavailable_reason.as_deref(),
        Some("history.policy_invalid")
    );
    assert!(
        settings
            .status
            .invalid_keys
            .iter()
            .any(|key| key == DAYS_KEY)
    );
    Ok(())
}

#[test]
fn invalid_days_reviews_leave_the_sqlite_setting_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("history-retention-write.db");
    let mut database = AppDb::open_or_create_gui(&path).map_err(|error| format!("{error:?}"))?;
    database
        .commit(
            0,
            &SettingsChanges::from([(DAYS_KEY.into(), Some(json!(180)))]),
            context(),
        )
        .map_err(|error| format!("{error:?}"))?;
    drop(database);

    let settings = StartupSettings::open(&test_database(&path)?, 100, &["en", "ko"])?;
    let service = settings
        .service
        .lock()
        .map_err(|_| "settings lock poisoned")?;
    let before = service.snapshot().clone();
    for value in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("180"),
        Value::Null,
        json!(u64::from(u32::MAX) + 1),
    ] {
        let changes = SettingsChanges::from([(DAYS_KEY.into(), Some(value))]);
        assert_eq!(
            service.review(before.revision, &changes),
            Err(du_user_env::settings::SettingsError::InvalidValue)
        );
        assert_eq!(service.snapshot(), &before);
    }
    drop(service);
    drop(settings);

    let mut database = AppDb::open_or_create_gui(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        database
            .load()
            .map_err(|error| format!("{error:?}"))?
            .values
            .get(DAYS_KEY),
        Some(&json!(180))
    );
    Ok(())
}
