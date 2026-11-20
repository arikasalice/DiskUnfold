//! #195: `app.startup_memory_set` with the real settings store.
use super::*;
use crate::{settings::StartupSettings, shared_database::SharedDatabase};
use du_app_db::AppDb;
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, EventEnvelope, Id, ResultEnvelope,
    settings::SettingsChanges,
    startup::{
        FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupCapabilitiesPort,
        StartupDialogOutcome, WebViewAvailability,
    },
};
use du_engine_api::api::Engine;
use serde_json::{Value, json};
use std::{path::PathBuf, thread, time::Duration};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Capabilities;

impl StartupCapabilitiesPort for Capabilities {
    fn full_disk_access_status(&self) -> FullDiskAccessStatus {
        FullDiskAccessStatus::NotApplicable
    }
    fn webview_availability(&self) -> WebViewAvailability {
        WebViewAvailability::Unknown
    }
    fn open_privacy_settings(&self) -> PrivacySettingsOpenOutcome {
        PrivacySettingsOpenOutcome::Unsupported
    }
    fn show_pre_window_error_dialog(&self, _: &str, _: &str) -> StartupDialogOutcome {
        StartupDialogOutcome::Unsupported
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
    settings: Arc<StartupSettings>,
    session: Arc<StartupAccessSession>,
    events: Arc<DesktopEvents>,
    engine: Engine,
}

fn open(path: &std::path::Path) -> Result<Arc<StartupSettings>, Box<dyn std::error::Error>> {
    let database = AppDb::open_or_create_gui(path).map_err(|error| format!("{error:?}"))?;
    let shared = SharedDatabase::new(database, None);
    Ok(Arc::new(StartupSettings::open(&shared, 1, &["en", "ko"])?))
}

fn fixture_at(directory: tempfile::TempDir) -> Result<Fixture, Box<dyn std::error::Error>> {
    let path = directory.path().join("startup-memory.db");
    let settings = open(&path)?;
    let session = Arc::new(StartupAccessSession::new(Arc::new(Capabilities)));
    let events = Arc::new(DesktopEvents::default());
    events.begin_document_generation();
    events.mark_document_ready();
    let mut builder = EngineBuilder::default();
    settings
        .clone()
        .register_startup_memory(&mut builder, events.clone(), session.clone());
    Ok(Fixture {
        _directory: directory,
        path,
        settings,
        session,
        events,
        engine: builder.build()?,
    })
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    fixture_at(tempfile::tempdir()?)
}

fn set(fixture: &Fixture, body: Value) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let correlation_id = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let caller = CallerCtx::new(Channel::Gui, correlation_id.clone(), None);
    Ok(fixture.engine.call(
        CallEnvelope {
            name: "app.startup_memory_set".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id,
            body,
        },
        &caller,
    ))
}

fn stored(fixture: &Fixture, key: &str) -> Option<Value> {
    fixture
        .settings
        .service
        .lock()
        .ok()
        .and_then(|service| service.snapshot().values.get(key).cloned())
}

fn reason(envelope: &ResultEnvelope) -> Option<&str> {
    envelope
        .error
        .as_ref()
        .map(|error| error.reason_key.as_str())
}

/// The `settings.changed` events published so far.
fn settings_events(
    events: &DesktopEvents,
) -> Result<Vec<EventEnvelope>, Box<dyn std::error::Error>> {
    thread::sleep(Duration::from_millis(125));
    let mut found = Vec::new();
    while let Some(wire) = events.next_json() {
        let event: EventEnvelope = serde_json::from_str(&wire)?;
        if event.topic == "settings.changed" {
            found.push(event);
        }
    }
    Ok(found)
}

#[test]
fn a_state_key_is_stored_published_answered_and_kept_across_launches() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = fixture_at(directory)?;
    let key = "grade:unverifiable|support_matrix:env_unknown";
    let reply = set(
        &fixture,
        json!({"kind": "notice_dismissed", "stateKey": key}),
    )?;
    assert!(reply.error.is_none(), "{reply:?}");
    assert_eq!(
        reply.body,
        json!({"kind": "notice_dismissed", "stateKey": key})
    );
    assert_eq!(stored(&fixture, NOTICE_DISMISSED), Some(json!(key)));
    assert_eq!(
        fixture.session.memories().notice_dismissed.as_deref(),
        Some(key)
    );
    assert_eq!(fixture.session.memories().review_seen, None);
    let events = settings_events(&fixture.events)?;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].body["changes"][0]["key"], json!(NOTICE_DISMISSED));
    assert_eq!(events[0].body["source"], json!("screen"));

    // A later launch loads the memory into its session before any answer.
    let path = fixture.path.clone();
    let Fixture { _directory, .. } = fixture;
    let settings = open(&path)?;
    let session = Arc::new(StartupAccessSession::new(Arc::new(Capabilities)));
    let mut builder = EngineBuilder::default();
    settings.register_startup_memory(
        &mut builder,
        Arc::new(DesktopEvents::default()),
        session.clone(),
    );
    assert_eq!(session.memories().notice_dismissed.as_deref(), Some(key));
    Ok(())
}

#[test]
fn invalid_requests_are_refused_and_store_nothing() -> TestResult {
    let fixture = fixture()?;
    for body in [
        json!({"kind": "notice_dismissed", "stateKey": "ok", "extra": 1}),
        json!({"kind": "future", "stateKey": "ok"}),
        json!({"kind": "review_seen", "stateKey": "Grade:Limited"}),
        json!({"kind": "review_seen", "stateKey": "grade limited"}),
        json!({"kind": "review_seen", "stateKey": "a".repeat(201)}),
    ] {
        let reply = set(&fixture, body.clone())?;
        assert_eq!(reason(&reply), Some("command.body_invalid"), "{body}");
    }
    assert_eq!(stored(&fixture, NOTICE_DISMISSED), None);
    assert_eq!(stored(&fixture, REVIEW_SEEN), None);
    assert!(settings_events(&fixture.events)?.is_empty());
    let longest = "a".repeat(200);
    let reply = set(
        &fixture,
        json!({"kind": "review_seen", "stateKey": longest}),
    )?;
    assert!(reply.error.is_none(), "{reply:?}");
    assert_eq!(stored(&fixture, REVIEW_SEEN), Some(json!(longest)));
    Ok(())
}

#[test]
fn the_same_key_again_writes_nothing_and_kinds_stay_apart() -> TestResult {
    let fixture = fixture()?;
    let body = json!({"kind": "review_seen", "stateKey": "grade:limited"});
    let first = set(&fixture, body.clone())?;
    let again = set(&fixture, body)?;
    assert_eq!(first.body, again.body);
    assert_eq!(settings_events(&fixture.events)?.len(), 1);
    set(
        &fixture,
        json!({"kind": "notice_dismissed", "stateKey": "grade:pass"}),
    )?;
    assert_eq!(stored(&fixture, REVIEW_SEEN), Some(json!("grade:limited")));
    assert_eq!(
        stored(&fixture, NOTICE_DISMISSED),
        Some(json!("grade:pass"))
    );
    Ok(())
}

#[test]
fn the_empty_key_clears_the_memory_back_to_the_default() -> TestResult {
    let fixture = fixture()?;
    set(
        &fixture,
        json!({"kind": "notice_dismissed", "stateKey": "grade:limited"}),
    )?;
    let reply = set(
        &fixture,
        json!({"kind": "notice_dismissed", "stateKey": ""}),
    )?;
    assert_eq!(
        reply.body,
        json!({"kind": "notice_dismissed", "stateKey": ""})
    );
    // The default is never stored, so the key has no value of its own.
    assert_eq!(stored(&fixture, NOTICE_DISMISSED), None);
    assert_eq!(fixture.session.memories().notice_dismissed, None);
    Ok(())
}

#[test]
fn a_busy_store_answers_busy_and_stores_nothing() -> TestResult {
    let fixture = fixture()?;
    let holder = rusqlite::Connection::open(&fixture.path)?;
    holder.execute_batch("BEGIN EXCLUSIVE;")?;
    let reply = set(
        &fixture,
        json!({"kind": "review_seen", "stateKey": "grade:limited"}),
    )?;
    holder.execute_batch("ROLLBACK;")?;
    assert_eq!(reason(&reply), Some("db.busy"));
    assert_eq!(stored(&fixture, REVIEW_SEEN), None);
    assert_eq!(fixture.session.memories().review_seen, None);
    Ok(())
}

#[test]
fn a_settings_review_taken_before_the_write_is_stale() -> TestResult {
    let fixture = fixture()?;
    let (revision, changes, reviewed) = {
        let service = fixture
            .settings
            .service
            .lock()
            .map_err(|_| "poisoned settings")?;
        let revision = service.snapshot().revision;
        let changes = SettingsChanges::from([("a11y.motion".to_owned(), Some(json!("reduce")))]);
        let reviewed = service
            .review(revision, &changes)
            .map_err(|error| format!("{error:?}"))?;
        (revision, changes, reviewed)
    };
    set(
        &fixture,
        json!({"kind": "review_seen", "stateKey": "grade:limited"}),
    )?;
    let mut service = fixture
        .settings
        .service
        .lock()
        .map_err(|_| "poisoned settings")?;
    let commit = SettingsCommit {
        source: SettingsSource::Screen,
        timestamp_ms: 2,
    };
    assert_eq!(
        service.apply(revision, &changes, reviewed, commit),
        Err(SettingsError::StaleVersion)
    );
    Ok(())
}
