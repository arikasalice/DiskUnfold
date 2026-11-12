use super::*;
use crate::{
    locale,
    settings::{LocaleRuntime, StartupSettings},
    shared_database::SharedDatabase,
};
use du_app_db::AppDb;
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode, EventEnvelope, Id, Timestamp,
    locale_ipc::LocaleSupportedLanguage,
    settings::{SettingsChanges, SettingsCommit, SettingsSource, SettingsStore},
    time_zone::{LocalTimeZone, TimeZoneId, TimeZonePort, TimeZoneUnavailable},
};
use du_engine_api::api::{CallerCtx, Channel, Engine, EngineBuilder};
use du_user_env::locale::OsLanguage;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, thread, time::Duration};
use tempfile::TempDir;

struct Fixture {
    _directory: TempDir,
    database_path: PathBuf,
    settings: Arc<StartupSettings>,
    startup_locale: Arc<StartupLocale>,
}

struct IdentifiedTimeZone;

impl TimeZonePort for IdentifiedTimeZone {
    fn observe(&self, _: &Timestamp) -> Result<LocalTimeZone, TimeZoneUnavailable> {
        Ok(LocalTimeZone {
            identity: Some(TimeZoneId::Windows("fixture-zone".into())),
            offset_seconds: 0,
        })
    }
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("locale-profile.db");
    let database =
        AppDb::open_or_create_gui(&database_path).map_err(|error| format!("{error:?}"))?;
    let shared = SharedDatabase::new(database, None);
    let settings = Arc::new(StartupSettings::open(&shared, 1, &["en", "ko"])?);
    let os_languages = ["en-US"];
    let startup_locale = Arc::new(locale::initialize_with_os(
        locale::register()?,
        None,
        OsLanguage::Supported(&os_languages),
    )?);
    Ok(Fixture {
        _directory: directory,
        database_path,
        settings,
        startup_locale,
    })
}

fn inventory() -> SupportedLanguageInventory {
    Arc::new(|| {
        Ok(vec![
            LocaleSupportedLanguage {
                language: "en".into(),
                self_name_key: "locale.language.en".into(),
                bundle_revision: "test-en-bundle-revision".into(),
            },
            LocaleSupportedLanguage {
                language: "ko".into(),
                self_name_key: "locale.language.ko".into(),
                bundle_revision: "test-ko-bundle-revision".into(),
            },
        ])
    })
}

fn engine(
    fixture: &Fixture,
    events: Arc<DesktopEvents>,
    primary_window: bool,
) -> Result<Engine, Box<dyn std::error::Error>> {
    let runtime = LocaleRuntime::new(
        fixture.startup_locale.clone(),
        events,
        inventory(),
        Arc::new(IdentifiedTimeZone),
    );
    let mut builder = EngineBuilder::default();
    fixture
        .settings
        .clone()
        .register_locale_profile(&mut builder, runtime, primary_window);
    Ok(builder.build()?)
}

fn call(
    engine: &Engine,
    name: &str,
    body: Value,
) -> Result<du_core_contracts::ResultEnvelope, Box<dyn std::error::Error>> {
    let correlation_id = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let caller = CallerCtx::new(Channel::Gui, correlation_id.clone(), None);
    Ok(engine.call(
        CallEnvelope {
            name: name.into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id,
            body,
        },
        &caller,
    ))
}

fn ready_events() -> Arc<DesktopEvents> {
    let events = Arc::new(DesktopEvents::default());
    events.begin_document_generation();
    events.mark_document_ready();
    events
}

fn next_event(events: &DesktopEvents) -> Result<EventEnvelope, Box<dyn std::error::Error>> {
    thread::sleep(Duration::from_millis(125));
    let wire = events.next_json().ok_or("missing desktop event")?;
    Ok(serde_json::from_str(&wire)?)
}

#[test]
fn profile_get_uses_os_source_and_explicit_same_language_persists_source_once()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture()?;
    let events = ready_events();
    let engine = engine(&fixture, events.clone(), true)?;

    let initial = call(&engine, "i18n.get_profile", json!({}))?;
    assert!(initial.error.is_none());
    assert_eq!(initial.body["displayLanguage"], "en");
    assert_eq!(initial.body["languageSource"], "os");
    assert_eq!(initial.body["profileRevision"], "0");
    assert_eq!(initial.body["supportedLanguages"][0]["language"], "en");
    assert_eq!(initial.body["supportedLanguages"][1]["language"], "ko");

    let explicit = call(
        &engine,
        "i18n.set_profile",
        json!({
            "expectedProfileRevision": "0",
            "changes": {"displayLanguage": "en"}
        }),
    )?;
    assert!(explicit.error.is_none());
    assert_eq!(explicit.body["languageSource"], "stored");
    assert_eq!(explicit.body["profileRevision"], "1");

    let settings = next_event(&events)?;
    assert_eq!(settings.topic, "settings.changed");
    assert_eq!(settings.seq, 1);
    assert_eq!(
        settings.body,
        json!({
            "previousSettingsRevision":"0", "settingsRevision":"1",
            "changes":[{"key":"locale.display_language", "applyTiming":"immediate"}],
            "source":"screen"
        })
    );
    let event = next_event(&events)?;
    assert_eq!(event.topic, "locale.changed");
    assert_eq!(event.seq, 2);
    assert_eq!(event.body["profileRevision"], "1");
    assert_eq!(event.body["changedFields"], json!(["display_language"]));
    assert_eq!(event.body["displayLanguage"], "en");

    let repeated = call(
        &engine,
        "i18n.set_profile",
        json!({
            "expectedProfileRevision": "1",
            "changes": {"displayLanguage": "en"}
        }),
    )?;
    assert!(repeated.error.is_none());
    assert_eq!(repeated.body["profileRevision"], "1");
    assert!(events.next_json().is_none());

    let stale = call(
        &engine,
        "i18n.set_profile",
        json!({
            "expectedProfileRevision": "0",
            "changes": {"dateFormat": "iso8601"}
        }),
    )?;
    assert_eq!(
        stale.error.as_ref().map(|error| error.reason_key.as_str()),
        Some("locale.profile_modified")
    );
    assert!(events.next_json().is_none());
    Ok(())
}

#[test]
fn stale_database_writer_is_rejected_by_durable_settings_compare_and_set()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture()?;
    let events = ready_events();
    let engine = engine(&fixture, events.clone(), true)?;
    let mut other_writer =
        AppDb::open_gui(&fixture.database_path).map_err(|error| format!("{error:?}"))?;
    other_writer
        .commit(
            0,
            &SettingsChanges::from([("locale.date_format".into(), Some(json!("iso8601")))]),
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 2,
            },
        )
        .map_err(|error| format!("{error:?}"))?;

    let response = call(
        &engine,
        "i18n.set_profile",
        json!({
            "expectedProfileRevision": "0",
            "changes": {"motion": "reduce"}
        }),
    )?;
    assert_eq!(
        response.error.as_ref().map(|error| &error.code),
        Some(&ErrorCode::StaleVersion)
    );
    assert_eq!(
        response
            .error
            .as_ref()
            .map(|error| error.reason_key.as_str()),
        Some("locale.profile_modified")
    );
    assert!(events.next_json().is_none());

    let persisted = other_writer.load().map_err(|error| format!("{error:?}"))?;
    assert_eq!(persisted.revision, 1);
    assert_eq!(persisted.values["locale.date_format"], json!("iso8601"));
    assert!(!persisted.values.contains_key("a11y.motion"));
    Ok(())
}

#[test]
fn non_primary_window_cannot_write_but_can_read_and_invalid_values_keep_the_profile()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = fixture()?;
    let events = ready_events();
    let secondary = engine(&fixture, events.clone(), false)?;
    let read = call(&secondary, "i18n.get_profile", json!({}))?;
    assert!(read.error.is_none());
    let denied = call(
        &secondary,
        "i18n.set_profile",
        json!({
            "expectedProfileRevision": "0",
            "changes": {"motion": "reduce"}
        }),
    )?;
    assert_eq!(
        denied.error.as_ref().map(|error| &error.code),
        Some(&ErrorCode::OriginDenied)
    );
    assert_eq!(
        denied.error.as_ref().map(|error| error.reason_key.as_str()),
        Some("command.window_denied")
    );

    let primary = engine(&fixture, events.clone(), true)?;
    for (changes, reason) in [
        (json!({"displayLanguage": "fr"}), "locale.unsupported"),
        (json!({"dateFormat": "unknown"}), "locale.format_unknown"),
        (json!({"motion": "fast"}), "settings.value_invalid"),
        (json!({"appScale": 99}), "settings.value_invalid"),
    ] {
        let invalid = call(
            &primary,
            "i18n.set_profile",
            json!({"expectedProfileRevision":"0", "changes":changes}),
        )?;
        assert_eq!(
            invalid
                .error
                .as_ref()
                .map(|error| error.reason_key.as_str()),
            Some(reason)
        );
    }
    assert!(events.next_json().is_none());
    Ok(())
}
