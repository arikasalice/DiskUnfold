//! The assembly registers the product visualization through the production path.
use super::{super::*, register};
use du_core_contracts::{
    Id, ResultEnvelope,
    startup::{
        FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupCapabilitiesPort,
        StartupDialogOutcome, WebViewAvailability,
    },
};
use serde_json::{Value, json};

#[cfg(any(windows, target_os = "linux"))]
mod real_scan;

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
    #[cfg(any(windows, target_os = "linux"))]
    events: Arc<DesktopEvents>,
    _profile: tempfile::TempDir,
}

pub(super) fn assemble() -> Result<Assembled, Failure> {
    let profile = tempfile::tempdir()?;
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
        #[cfg(any(windows, target_os = "linux"))]
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
    assert!(second.is_err());
    Ok(())
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
