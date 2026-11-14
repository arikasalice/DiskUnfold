//! Compose the desktop analysis flow from one platform's collection adapters.
use std::sync::Arc;

use du_core_contracts::{
    scan_observation::ScopeRootPort, startup::StartupCapabilitiesPort, volume::VolumePort,
};
use du_engine_api::{
    api::EngineBuilder,
    desktop::{self, DesktopEvents},
};

#[cfg(windows)]
use crate::recent_scope_worker::RecentScopeWorker;
use crate::{
    settings::StartupSettings, shared_database::SharedDatabase,
    startup_checks::report_pre_window_error,
};

/// Read-only collection adapters that one platform supplies to the analysis flow.
#[cfg_attr(not(windows), allow(dead_code))]
struct ScanPorts {
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
}

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
    #[cfg(not(windows))]
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
    let history = Arc::new(database.history(crate::settings::current_time_ms));
    let random = &du_platform_os::entropy::OsRandom;
    let scan_limit = settings.clone().scan_limit_source();
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
    let services = match recent_capacity {
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
    .map_err(|error| report_pre_window_error(capabilities, error))?;
    crate::group_rules_store::register(builder, services.groups, database, capabilities)?;
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
