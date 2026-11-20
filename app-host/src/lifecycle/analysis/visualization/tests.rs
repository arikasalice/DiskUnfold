//! The assembly registers the product visualization through the production path.
use super::{super::*, register};
use du_core_contracts::{
    Id, ResultEnvelope,
    startup::{
        FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupCapabilitiesPort,
        StartupDialogOutcome, WebViewAvailability,
    },
};
use du_engine_api::{
    api::{CallerCtx, Channel, UseCase},
    error::EngineError,
};
use serde_json::{Value, json};

#[cfg(target_os = "macos")]
mod real_device_probe;
#[cfg(any(windows, target_os = "linux"))]
mod real_scan;
#[cfg(any(windows, target_os = "linux"))]
mod real_scan_allocated;
#[cfg(windows)]
mod real_scan_links;

type Failure = Box<dyn std::error::Error>;

const VIZ_COMMANDS: [&str; 9] = [
    "viz.view.open",
    "viz.view.close",
    "viz.render",
    "viz.cancel",
    "viz.rows",
    "viz.group_rows",
    "viz.locate",
    "viz.catalog",
    "viz.availability",
];

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

/// The engine the production assembly function builds, with its event queue.
pub(super) struct Assembled {
    engine: Engine,
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    events: Arc<DesktopEvents>,
    _profile: tempfile::TempDir,
}

pub(super) fn assemble() -> Result<Assembled, Failure> {
    let profile = tempfile::tempdir()?;
    assemble_in(profile)
}

/// Assembles over a profile whose settings already store `rule_id` for the size rule.
#[cfg(any(windows, target_os = "linux"))]
pub(super) fn assemble_with_stored_rule(rule_id: &str) -> Result<Assembled, Failure> {
    use du_core_contracts::settings::SettingsStore as _;
    let profile = tempfile::tempdir()?;
    let path = profile.path().join("diskunfold.db");
    let mut db = du_app_db::AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    let stored = du_core_contracts::settings::SettingsChanges::from([(
        du_user_env::space::AGGREGATION_RULE.to_owned(),
        Some(json!(rule_id)),
    )]);
    let context = du_core_contracts::settings::SettingsCommit {
        source: du_core_contracts::settings::SettingsSource::Screen,
        timestamp_ms: 1,
    };
    db.commit(0, &stored, context)
        .map_err(|e| format!("{e:?}"))?;
    drop(db);
    assemble_in(profile)
}

fn assemble_in(profile: tempfile::TempDir) -> Result<Assembled, Failure> {
    let database = crate::startup_database::open(profile.path())?;
    let settings = Arc::new(StartupSettings::open(&database, 1, &["en", "ko"])?);
    let mut builder = EngineBuilder::default();
    let desktop = register_desktop(
        &mut builder,
        &Capabilities,
        &settings,
        &database,
        #[cfg(windows)]
        None,
    )?;
    let engine = builder.build()?;
    verify_registered(&engine, desktop.analysis_registered)?;
    Ok(Assembled {
        engine,
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        events: desktop.events,
        _profile: profile,
    })
}

pub(super) fn ipc(engine: &Engine, name: &str, body: Value) -> Result<ResultEnvelope, Failure> {
    let id = Id::from_v7_bytes([42; 16]);
    let message = json!({
        "command": "du_call",
        "args": {"envelope": {
            "name": name, "contractVersion": 1, "correlationId": id, "body": body
        }}
    });
    let response = crate::ipc::dispatch(engine, &message.to_string()).ok_or("no response")?;
    Ok(serde_json::from_str(&response)?)
}

pub(super) fn ok(engine: &Engine, name: &str, body: Value) -> Result<Value, Failure> {
    let response = ipc(engine, name, body)?;
    match response.error {
        Some(error) => Err(format!("{name}: {}", error.reason_key).into()),
        None => Ok(response.body),
    }
}

#[test]
fn the_desktop_assembly_registers_every_required_visualization_command() -> Result<(), Failure> {
    let assembled = assemble()?;
    for name in VIZ_COMMANDS {
        assert!(assembled.engine.has_use_case(name), "{name}");
        assert!(REQUIRED_USE_CASES.contains(&name), "{name}");
    }
    let catalog = ok(&assembled.engine, "viz.catalog", json!({}))?;
    assert_eq!(catalog["formats"].as_array().map(Vec::len), Some(6));
    assert_eq!(catalog["strategies"].as_array().map(Vec::len), Some(6));
    assert_eq!(catalog["colorCriteria"], json!(["none"]));
    Ok(())
}

#[test]
fn a_missing_visualization_use_case_stops_startup() -> Result<(), Failure> {
    let profile = tempfile::tempdir()?;
    let database = crate::startup_database::open(profile.path())?;
    let ports = scan_ports().ok_or("no collection adapters on this platform")?;
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(du_engine_api::desktop::SharedTableLimit::default);
    // Scanning alone leaves out the visualization commands.
    register_analysis(&mut builder, ports, &database, limit, None)?;
    let engine = builder.build()?;
    let error = verify_registered(&engine, true)
        .err()
        .ok_or("a missing visualization registration was accepted")?;
    assert_eq!(
        error.to_string(),
        "startup.registration_failed: missing use case viz.view.open"
    );
    Ok(())
}

#[test]
fn a_second_visualization_registration_is_an_error_that_stops_startup() -> Result<(), Failure> {
    let profile = tempfile::tempdir()?;
    let database = crate::startup_database::open(profile.path())?;
    let settings = Arc::new(StartupSettings::open(&database, 1, &["en", "ko"])?);
    let ports = scan_ports().ok_or("no collection adapters on this platform")?;
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(du_engine_api::desktop::SharedTableLimit::default);
    let services = register_analysis(&mut builder, ports, &database, limit, None)?;
    register(
        &services.visualization,
        &mut builder,
        settings.clone().unit_table_source(),
    )?;
    let second = register(
        &services.visualization,
        &mut builder,
        settings.unit_table_source(),
    );
    assert_eq!(second, Err("visualization job registration failed"));
    Ok(())
}

#[test]
fn a_viz_command_registered_by_another_owner_stops_the_engine_build() -> Result<(), Failure> {
    // `register_desktop` reports a failed visualization registration itself, but no input
    // makes it fail from outside: the job runtime and the strategies are built inside.
    // A command name taken by another owner is the failure a caller can cause, and the
    // engine build refuses it, which the lifecycle reports as a startup failure.
    let profile = tempfile::tempdir()?;
    let database = crate::startup_database::open(profile.path())?;
    let settings = Arc::new(StartupSettings::open(&database, 1, &["en", "ko"])?);
    let mut builder = EngineBuilder::default();
    builder.register(Squatter);
    register_desktop(
        &mut builder,
        &Capabilities,
        &settings,
        &database,
        #[cfg(windows)]
        None,
    )?;
    let error = builder
        .build()
        .err()
        .ok_or("a duplicate visualization command was accepted")?;
    assert_eq!(error.duplicate_name, "viz.catalog");
    Ok(())
}

/// A use case that already owns a visualization command name.
struct Squatter;

impl UseCase for Squatter {
    const NAME: &'static str = "viz.catalog";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = Value;
    type Response = Value;

    fn check_scope(&self, _: &Value, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Value, _: &CallerCtx) -> Result<Value, EngineError> {
        Ok(json!({}))
    }
}

#[test]
fn the_visualization_message_keys_are_translated_in_both_languages() -> Result<(), Failure> {
    use du_engine_api::viz_production::policy;
    let registry =
        du_user_env::i18n::embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    for (key, en, ko) in [
        (
            policy::LEGEND_LABEL_UNCLASSIFIED,
            "Unclassified",
            "분류 없음",
        ),
        (policy::GROUP_OTHER_NAME_KEY, "Other items", "기타 항목"),
    ] {
        assert_eq!(
            registry.resolve_plain_text("en", key).as_deref(),
            Some(en),
            "{key}"
        );
        assert_eq!(
            registry.resolve_plain_text("ko", key).as_deref(),
            Some(ko),
            "{key}"
        );
    }
    Ok(())
}
