use super::*;
use crate::{locale, settings::StartupSettings, shared_database::SharedDatabase};
use du_app_db::AppDb;
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id,
    settings::{SettingsChanges, SettingsCommit, SettingsSource, SettingsStore},
};
use du_engine_api::api::{Engine, EngineBuilder};
use du_platform_os::macos_time_zone::MacOsTimeZone;
use du_user_env::locale::OsLanguage;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use tempfile::TempDir;

struct Fixture {
    _directory: TempDir,
    database_path: PathBuf,
    settings: Arc<StartupSettings>,
    startup_locale: Arc<crate::locale::StartupLocale>,
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("locale-profile.db");
    let mut database =
        AppDb::open_or_create_gui(&database_path).map_err(|error| format!("{error:?}"))?;
    database
        .commit(
            0,
            &SettingsChanges::from([(
                du_user_env::locale::DISPLAY_LANGUAGE_KEY.into(),
                Some(json!("ko")),
            )]),
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 1,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    let shared = SharedDatabase::new(database, None);
    let settings = Arc::new(StartupSettings::open(&shared, 2, &["en", "ko"])?);
    let stored = settings
        .stored_language()?
        .ok_or("saved locale setting was not restored")?;
    let startup_locale = Arc::new(locale::initialize_with_os(
        locale::register()?,
        Some(&stored),
        OsLanguage::Supported(&["en-US"]),
    )?);
    Ok(Fixture {
        _directory: directory,
        database_path,
        settings,
        startup_locale,
    })
}

/// Status the profile must report for this machine's own time zone.
///
/// Hosted runners use a fixed `GMT` zone, which stays offset-only by design;
/// a Mac set to a named zone reports `identified`. The name classification is
/// pinned by the platform-os unit tests.
fn expected_time_zone_status() -> &'static str {
    let Some(now) = du_core_contracts::Timestamp::from_unix_millis(current_time_ms()) else {
        return "unavailable";
    };
    match MacOsTimeZone.observe(&now) {
        Ok(observation) if observation.identity.is_some() => "identified",
        Ok(_) => "offset_only",
        Err(_) => "unavailable",
    }
}

fn call(engine: &Engine, name: &str, body: Value) -> Result<Value, Box<dyn std::error::Error>> {
    let correlation_id = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let envelope = CallEnvelope {
        name: name.to_owned(),
        contract_version: ENVELOPE_VERSION,
        correlation_id,
        body,
    };
    let message = json!({"command":"du_call", "args":{"envelope":envelope}}).to_string();
    let response = crate::ipc::dispatch(engine, &message).ok_or("missing IPC response")?;
    Ok(serde_json::from_str(&response)?)
}

#[test]
fn mac_startup_serves_saved_locale_read_only_through_gui_ipc()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture()?;
    let inventory = fixture.startup_locale.clone();
    let runtime = LocaleRuntime::read_only(
        fixture.startup_locale.clone(),
        Arc::new(move || Ok(inventory.registry().supported_languages())),
        Arc::new(MacOsTimeZone),
    );
    let mut builder = EngineBuilder::default();
    fixture
        .settings
        .clone()
        .register_locale_profile_read_only(&mut builder, runtime);
    crate::locale_delivery::register(&mut builder, fixture.startup_locale.clone());
    let engine = builder.build()?;

    let profile = call(&engine, "i18n.get_profile", json!({}))?;
    assert_eq!(profile["body"]["displayLanguage"], "ko");
    assert_eq!(profile["body"]["languageSource"], "stored");
    assert_eq!(profile["body"]["profileRevision"], "1");
    assert_eq!(
        profile["body"]["localTimeZoneStatus"],
        expected_time_zone_status()
    );

    let bundle = call(&engine, "i18n.get_bundle", json!({"language":"ko"}))?;
    assert!(bundle["error"].is_null(), "bundle query failed: {bundle}");
    assert_eq!(
        bundle["body"]["bundle"]["messages"]["ui.startup.title"]["template"],
        "시작 상태와 개인정보 보호"
    );

    let mutation = call(
        &engine,
        "i18n.set_profile",
        json!({"expectedProfileRevision":"1", "changes":{"displayLanguage":"en"}}),
    )?;
    assert_eq!(mutation["error"]["reasonKey"], "command.not_provided");

    let mut reopened =
        AppDb::open_read_only(&fixture.database_path).map_err(|error| format!("{error:?}"))?;
    let persisted = reopened.load().map_err(|error| format!("{error:?}"))?;
    assert_eq!(persisted.revision, 1);
    assert_eq!(
        persisted
            .values
            .get(du_user_env::locale::DISPLAY_LANGUAGE_KEY),
        Some(&json!("ko"))
    );
    Ok(())
}
