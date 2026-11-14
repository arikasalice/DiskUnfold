//! Compose the desktop analysis flow from one platform's collection adapters.
use std::sync::Arc;

use du_core_contracts::{
    scan_observation::ScopeRootPort, startup::StartupCapabilitiesPort, volume::VolumePort,
};
use du_engine_api::{
    api::{Engine, EngineBuilder},
    desktop::{self, DesktopEvents, DesktopServices, SharedTableLimitSource},
};

#[cfg(windows)]
use crate::recent_scope_worker::RecentScopeWorker;
use crate::{
    settings::StartupSettings, shared_database::SharedDatabase,
    startup_checks::report_pre_window_error,
};

/// Read-only collection adapters that one platform supplies to the analysis flow.
#[derive(Clone)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
struct ScanPorts {
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
}

/// Use cases that must exist once the analysis flow is registered.
///
/// FR1.HD2.app-host.DD1: engine-api registers the `scan.metadata` job kind in
/// the same step as `scan.start`, which supplies its job bodies.
const REQUIRED_USE_CASES: [&str; 2] = ["scan.start", "scan.failures"];

/// The single per-platform injection point for scan collection adapters.
///
/// `None` leaves the analysis flow (scope review, targets, scans and their
/// queries) unregistered on this platform.
fn scan_ports() -> Option<ScanPorts> {
    #[cfg(windows)]
    {
        Some(ScanPorts {
            roots: Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
            volumes: Arc::new(du_platform_os::volume::WindowsVolumes),
        })
    }
    #[cfg(target_os = "linux")]
    {
        Some(ScanPorts {
            roots: Arc::new(du_platform_os::linux_observation::LinuxScopeRoots),
            volumes: Arc::new(du_platform_os::volume::LinuxVolumes),
        })
    }
    // macOS: return the platform-os scope root and volume adapters here once
    // they exist; no other assembly change is needed to register scanning.
    #[cfg(target_os = "macos")]
    {
        None
    }
}

/// Desktop analysis resources owned by the window lifecycle.
pub(super) struct Desktop {
    /// Window event stream; it carries no analysis events when the flow is unregistered.
    pub(super) events: Arc<DesktopEvents>,
    /// Whether the analysis flow and its job history writer were registered.
    pub(super) analysis_registered: bool,
    /// Recent-scope storage worker, present only when the caller opted in.
    #[cfg(windows)]
    pub(super) recent_worker: Option<RecentScopeWorker>,
}

/// Registers the analysis flow when this platform supplies collection adapters.
///
/// Registration failures are shown through the pre-window dialog and stop startup.
pub(super) fn register_desktop(
    builder: &mut EngineBuilder,
    capabilities: &dyn StartupCapabilitiesPort,
    settings: &Arc<StartupSettings>,
    database: &SharedDatabase,
    #[cfg(windows)] recent: Option<super::RecentScopeOptions>,
) -> Result<Desktop, Box<dyn std::error::Error>> {
    let Some(ports) = scan_ports() else {
        return Ok(Desktop {
            events: Arc::new(DesktopEvents::default()),
            analysis_registered: false,
            #[cfg(windows)]
            recent_worker: None,
        });
    };
    #[cfg(windows)]
    let recent_capacity = recent.as_ref().map(|_| {
        crate::recent_scope_open::register(
            builder,
            database.clone(),
            ports.roots.clone(),
            Arc::new(du_platform_os::volume::WindowsVolumes),
        );
        // Bound retained handles independently from the number of persisted rows.
        const PENDING_ADMISSIONS: std::num::NonZeroUsize = std::num::NonZeroUsize::new(32).unwrap();
        PENDING_ADMISSIONS
    });
    #[cfg(not(windows))]
    let recent_capacity = None;
    let services = register_analysis(
        builder,
        ports,
        database,
        settings.clone().scan_limit_source(),
        recent_capacity,
    )
    .map_err(|error| report_pre_window_error(capabilities, error))?;
    #[cfg(windows)]
    let recent_worker = recent
        .map(|options| {
            let admissions = services
                .recent_scope_admissions
                .ok_or_else(|| std::io::Error::other("recent scope admission queue unavailable"))?;
            crate::recent_scope_list::register(builder, database.clone(), options.cap);
            RecentScopeWorker::start(admissions, database.clone(), options.cap, options.warning)
        })
        .transpose()
        .map_err(|error| report_pre_window_error(capabilities, error))?;
    #[cfg(windows)]
    {
        use du_platform_os::windows_clipboard::WindowsClipboard;
        builder.register(crate::clipboard::CopyPath::new(
            services.path_copy,
            WindowsClipboard,
            |path: &du_core_contracts::result::ResultNativePath| WindowsClipboard.path_text(path),
            "\r\n",
        ));
    }
    Ok(Desktop {
        events: services.events,
        analysis_registered: true,
        #[cfg(windows)]
        recent_worker,
    })
}

/// Registers the engine analysis flow once with this platform's adapters.
///
/// The job history writer shares the GUI database connection.
fn register_analysis(
    builder: &mut EngineBuilder,
    ports: ScanPorts,
    database: &SharedDatabase,
    scan_limit: SharedTableLimitSource,
    recent_capacity: Option<std::num::NonZeroUsize>,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    let history = Arc::new(database.history(crate::settings::current_time_ms));
    let random = &du_platform_os::entropy::OsRandom;
    match recent_capacity {
        Some(capacity) => desktop::register_services_with_recent_scope_worker(
            builder,
            ports.roots,
            ports.volumes,
            history,
            random,
            scan_limit,
            capacity,
        ),
        None => desktop::register_services(
            builder,
            ports.roots,
            ports.volumes,
            history,
            random,
            scan_limit,
        ),
    }
}

/// Stops startup when a registered analysis flow lacks a required use case.
///
/// Duplicate registrations already fail in engine-api or when the engine is built.
pub(super) fn verify_registered(
    engine: &Engine,
    analysis_registered: bool,
) -> Result<(), std::io::Error> {
    if !analysis_registered {
        return Ok(());
    }
    match REQUIRED_USE_CASES
        .iter()
        .find(|name| !engine.has_use_case(name))
    {
        Some(name) => Err(std::io::Error::other(format!(
            "startup.registration_failed: missing use case {name}"
        ))),
        None => Ok(()),
    }
}

/// Marks jobs left running by an earlier session as interrupted.
///
/// Runs only where the analysis flow registered its history writer, and queues
/// the committed notice before the window is ready.
pub(super) fn recover_history(
    analysis_registered: bool,
    events: &DesktopEvents,
    database: &SharedDatabase,
    capabilities: &dyn StartupCapabilitiesPort,
) -> Result<(), std::io::Error> {
    if !analysis_registered {
        return Ok(());
    }
    events
        .recover_history(database, crate::settings::current_time_ms())
        .map_err(std::io::Error::other)
        .map_err(|error| report_pre_window_error(capabilities, error))
}

/// Registers the job history list, detail and item reads on their own connection.
#[cfg(windows)]
pub(super) fn register_history_reads(
    builder: &mut EngineBuilder,
    capabilities: &dyn StartupCapabilitiesPort,
    reader: crate::history::HistoryReader,
) -> Result<(), std::io::Error> {
    let report = |error| report_pre_window_error(capabilities, error);
    du_engine_api::history::detail::register(builder, reader.clone());
    du_engine_api::history::items::register(
        builder,
        reader.clone(),
        &du_platform_os::entropy::OsRandom,
    )
    .map_err(|error| std::io::Error::other(format!("history.storage_unavailable: {error:?}")))
    .map_err(report)?;
    du_engine_api::history::register(builder, reader, &du_platform_os::entropy::OsRandom)
        .map_err(|error| std::io::Error::other(format!("history.storage_unavailable: {error:?}")))
        .map_err(report)
}

#[cfg(all(test, target_os = "linux"))]
mod linux_scan_tests;
#[cfg(test)]
mod tests;
