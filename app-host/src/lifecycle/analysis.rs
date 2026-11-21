//! Compose the desktop analysis flow from one platform's collection adapters.
use std::sync::Arc;

use du_core_contracts::{
    scan_observation::ScopeRootPort, startup::StartupCapabilitiesPort, volume::VolumePort,
};
use du_engine_api::{
    api::{Engine, EngineBuilder},
    desktop::{self, DesktopEvents, DesktopServices, DesktopSources, SharedTableLimitSource},
};

#[cfg(windows)]
use crate::recent_scope_worker::RecentScopeWorker;
use crate::{
    settings::StartupSettings, shared_database::SharedDatabase,
    startup_checks::report_pre_window_error,
};

/// Read-only collection adapters that one platform supplies to the analysis flow.
#[derive(Clone)]
struct ScanPorts {
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
}

type HistoryRetentionRegistration = (
    crate::history_retention::HistoryExpiryReader,
    Arc<dyn du_core_contracts::job::SessionUndoInfo>,
    Arc<dyn Fn(bool) + Send + Sync>,
);

/// Use cases that must exist once the analysis flow is registered.
///
/// FR1.HD2.app-host.DD1: engine-api registers the `scan.metadata` job kind in
/// the same step as `scan.start`, which supplies its job bodies.
/// FR10.HD1.app-host.DD2: the visualization registration adds the `viz.*` commands
/// and the `viz.compute` job kind.
const REQUIRED_USE_CASES: [&str; 18] = [
    "scan.start",
    "scan.failures",
    "viz.view.open",
    "viz.view.close",
    "viz.render",
    "viz.cancel",
    "viz.rows",
    "viz.group_rows",
    "viz.locate",
    "viz.catalog",
    "viz.availability",
    "plan.draft_open",
    "plan.draft_edit",
    "plan.draft_cancel",
    "plan.build",
    "plan.get",
    "plan.item_reason",
    "plan.history",
];

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
    #[cfg(target_os = "macos")]
    {
        Some(ScanPorts {
            roots: Arc::new(du_platform_os::macos_observation::MacScopeRoots),
            volumes: Arc::new(du_platform_os::volume::MacVolumes),
        })
    }
}

/// Desktop analysis resources owned by the window lifecycle.
pub(super) struct Desktop {
    /// Window event stream; it carries no analysis events when the flow is unregistered.
    pub(super) events: Arc<DesktopEvents>,
    /// Whether the analysis flow and its job history writer were registered.
    pub(super) analysis_registered: bool,
    /// Submitter for the startup and periodic retention triggers.
    pub(super) history_prune_trigger:
        Option<du_engine_api::history::retention::HistoryPruneTrigger>,
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
    retention: Option<HistoryRetentionRegistration>,
    #[cfg(windows)] recent: Option<super::RecentScopeOptions>,
) -> Result<Desktop, Box<dyn std::error::Error>> {
    let Some(ports) = scan_ports() else {
        return Ok(Desktop {
            events: Arc::new(DesktopEvents::default()),
            analysis_registered: false,
            history_prune_trigger: None,
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
        // Bound retained handles independently from the number of persisted rows: 32.
        const PENDING_ADMISSIONS: std::num::NonZeroUsize =
            std::num::NonZeroUsize::MIN.saturating_add(31);
        PENDING_ADMISSIONS
    });
    #[cfg(not(windows))]
    let recent_capacity = None;
    let size_rules = settings.clone().size_rules();
    let services = register_analysis_with_rules(
        builder,
        ports,
        database,
        settings.clone().scan_limit_source(),
        size_rules,
        recent_capacity,
    )
    .map_err(|error| report_pre_window_error(capabilities, error))?;
    crate::snapshot_save::register(&services, builder, database)
        .map_err(|error| report_pre_window_error(capabilities, error))?;
    let history_prune_trigger = if let Some((expiry_reader, undo_info, policy_changed)) = retention
    {
        Some(
            services.register_history_retention(
                builder,
                settings.clone().history_retention_policy_source(),
                settings
                    .clone()
                    .history_retention_policy_saver(policy_changed.clone()),
                expiry_reader,
                undo_info,
                Arc::new(crate::history_retention::HistoryPruneWriter::new(
                    settings.clone(),
                    database.clone(),
                )),
            )?,
        )
    } else {
        None
    };
    services
        .bind_scope_observation_log(Arc::new(database.scope_observations()))
        .map_err(std::io::Error::other)
        .map_err(|error| report_pre_window_error(capabilities, error))?;
    visualization::register(
        &services.visualization,
        builder,
        settings.clone().unit_table_source(),
    )
    .map_err(|error| {
        report_pre_window_error(
            capabilities,
            std::io::Error::other(format!("startup.registration_failed: {error}")),
        )
    })?;
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
        history_prune_trigger,
        #[cfg(windows)]
        recent_worker,
    })
}

/// Registers the engine analysis flow once with this platform's adapters.
///
/// The job history writer shares the GUI database connection. The size rules are the
/// default bundle with the default rule ID.
#[cfg(test)]
fn register_analysis(
    builder: &mut EngineBuilder,
    ports: ScanPorts,
    database: &SharedDatabase,
    scan_limit: SharedTableLimitSource,
    recent_capacity: Option<std::num::NonZeroUsize>,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    let size_rules = size_rules::assemble(du_engine_api::size_rule::default_size_rule_source())?;
    register_analysis_with_rules(
        builder,
        ports,
        database,
        scan_limit,
        size_rules,
        recent_capacity,
    )
}

/// Registers the engine analysis flow once with this platform's adapters and an
/// already assembled size rule registry.
///
/// The job history writer shares the GUI database connection.
fn register_analysis_with_rules(
    builder: &mut EngineBuilder,
    ports: ScanPorts,
    database: &SharedDatabase,
    scan_limit: SharedTableLimitSource,
    size_rules: du_engine_api::size_rule::SizeRules,
    recent_capacity: Option<std::num::NonZeroUsize>,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    register_analysis_with_plan_source(
        builder,
        ports,
        database,
        scan_limit,
        size_rules,
        recent_capacity,
        None,
    )
}

#[cfg(test)]
fn register_analysis_with_test_plan_sources(
    builder: &mut EngineBuilder,
    ports: ScanPorts,
    database: &SharedDatabase,
    scan_limit: SharedTableLimitSource,
    size_rules: du_engine_api::size_rule::SizeRules,
    recent_capacity: Option<std::num::NonZeroUsize>,
    plan_sources: du_engine_api::plan::PlanSources,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    register_analysis_with_plan_source(
        builder,
        ports,
        database,
        scan_limit,
        size_rules,
        recent_capacity,
        Some(plan_sources),
    )
}

fn register_analysis_with_plan_source(
    builder: &mut EngineBuilder,
    ports: ScanPorts,
    database: &SharedDatabase,
    scan_limit: SharedTableLimitSource,
    size_rules: du_engine_api::size_rule::SizeRules,
    recent_capacity: Option<std::num::NonZeroUsize>,
    plan_sources: Option<du_engine_api::plan::PlanSources>,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    let history = Arc::new(database.history(crate::settings::current_time_ms));
    let random = &du_platform_os::entropy::OsRandom;
    let services = desktop::register_services_with_sources(
        builder,
        ports.roots,
        ports.volumes,
        history,
        random,
        DesktopSources {
            shared_table_limit: scan_limit,
            size_rules,
            recent_admission_capacity: recent_capacity,
        },
    )?;
    let plan_sources = plan_sources.unwrap_or_else(|| {
        let versions: du_engine_api::plan::SharedVersionStore =
            Arc::new(std::sync::Mutex::new(database.clone()));
        du_engine_api::plan::PlanSources::new(versions, Arc::new(database.clone()))
            .with_protection_facts(Arc::new(
                du_platform_os::protection_facts::SystemProtectionFacts,
            ))
    });
    let approval_prepare = services.register_plans(builder, plan_sources)?;
    crate::approval::register(
        builder,
        approval_prepare,
        database.clone(),
        services.events.clone(),
        Arc::new(du_platform_os::entropy::OsRandom),
        true,
    );
    Ok(services)
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

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod ipc_calls;
#[cfg(all(test, target_os = "linux"))]
mod linux_scan_tests;
#[cfg(all(test, target_os = "macos"))]
mod macos_scan_tests;
#[cfg(all(test, any(windows, target_os = "linux", target_os = "macos")))]
mod native_cancel_seal_tests;
#[cfg(test)]
mod plan_protection_tests;
#[cfg(test)]
mod plan_test_support;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod size_rules;
#[cfg(test)]
mod tests;
mod visualization;
