//! Assemble the GUI process and run its window lifecycle.
use std::sync::{Arc, Mutex};

use du_core_contracts::startup::StartupCapabilitiesPort;
use du_engine_api::api::EngineBuilder;
use du_platform_os::startup::SystemStartupCapabilities;
use du_platform_os::webview::WebApp;

#[cfg(windows)]
use crate::recent_scope_worker::{RecentScopeWorker, RecentScopeWriteWarning};
#[cfg(windows)]
use std::num::NonZeroUsize;

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod analysis;
mod assets;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod startup_state;

#[cfg(windows)]
const RECENT_SCOPE_BOOTSTRAP: &str = r#"
if (window === window.top && location.href === 'https://diskunfold.invalid/index.html') {
    Object.defineProperty(window, '__diskunfoldRecentScopes', { value: true });
}
"#;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_app(
        #[cfg(windows)]
        None,
        Arc::new(SystemStartupCapabilities),
        crate::product_ipc::unavailable_product_info(),
    )
}

/// Runs the GUI with the product metadata embedded by the launcher build.
pub fn run_with_product_info(
    product: du_core_contracts::product_support::EmbeddedProductInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    run_app(
        #[cfg(windows)]
        None,
        Arc::new(SystemStartupCapabilities),
        product,
    )
}

/// Runs the GUI with product metadata JSON embedded by the launcher build.
///
/// Metadata that does not parse stops startup with `product.metadata_invalid`
/// so a broken build cannot present invented product values.
pub fn run_with_product_info_json(product_json: &str) -> Result<(), Box<dyn std::error::Error>> {
    run_with_product_info(crate::product_ipc::parse_embedded(product_json)?)
}

/// Runs the Windows GUI with an explicitly supplied recent-scope retention policy.
/// The warning sink runs on the background storage thread and receives no path data.
#[cfg(windows)]
pub fn run_with_recent_scope(
    cap: NonZeroUsize,
    warning: impl Fn(RecentScopeWriteWarning) + Send + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    run_app(
        Some(RecentScopeOptions {
            cap,
            warning: Box::new(warning),
        }),
        Arc::new(SystemStartupCapabilities),
        crate::product_ipc::unavailable_product_info(),
    )
}

#[cfg(windows)]
struct RecentScopeOptions {
    cap: NonZeroUsize,
    warning: Box<dyn Fn(RecentScopeWriteWarning) + Send>,
}

fn run_app(
    #[cfg(windows)] recent: Option<RecentScopeOptions>,
    capabilities: Arc<dyn StartupCapabilitiesPort>,
    product: du_core_contracts::product_support::EmbeddedProductInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    // The launcher invokes this function on its UI thread; pre-window native dialogs
    // use that same thread before any platform webview is constructed.
    if let Err(error) = crate::startup_checks::validate(&crate::startup_checks::REGISTRATIONS) {
        let error = std::io::Error::other(format!("startup.check_table_invalid: {error:?}"));
        return Err(Box::new(crate::startup_checks::report_pre_window_error(
            capabilities.as_ref(),
            error,
        )));
    }
    let observed = crate::startup_checks::host_observations(
        capabilities.as_ref(),
        assets::MANIFEST,
        assets::ASSETS,
        assets::ASSETS_SHA256,
        &product,
    )?;
    let startup_access = Arc::new(crate::startup_checks::StartupAccessSession::new(
        capabilities.clone(),
    ));
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    let mut startup_observed = observed;
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    let state = startup_state::open(capabilities.as_ref(), &mut startup_observed)?;
    #[cfg(windows)]
    let history_reader =
        crate::history::HistoryReader::open(&state.data_dir, state.database.clone()).map_err(
            |error| crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error),
        )?;
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    let startup_state::StartupState {
        locale,
        settings,
        database,
        first_run,
        ..
    } = state;
    #[allow(unused_mut)]
    let mut builder = EngineBuilder::default();
    crate::product_ipc::register(
        &mut builder,
        crate::product_ipc::ProductServices::system(product),
    );
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    {
        startup_observed.push(crate::startup_checks::host_temporary_files_observation());
        let startup_status =
            crate::startup_checks::status(first_run, startup_access.current(), &startup_observed)
                .map_err(|error| {
                crate::startup_checks::report_pre_window_error(
                    capabilities.as_ref(),
                    std::io::Error::other(format!("startup.check_blocked: {error:?}")),
                )
            })?;
        crate::startup_checks::register(&mut builder, startup_status, startup_access.clone());
    }
    #[cfg(windows)]
    settings.register_restore_status(&mut builder);
    #[cfg(windows)]
    settings.clone().register_display_preferences(&mut builder);
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    let analysis::Desktop {
        events,
        analysis_registered,
        #[cfg(windows)]
        recent_worker,
    } = analysis::register_desktop(
        &mut builder,
        capabilities.as_ref(),
        &settings,
        &database,
        #[cfg(windows)]
        recent,
    )?;
    #[cfg(any(windows, target_os = "linux"))]
    {
        let inventory = locale.clone();
        #[cfg(windows)]
        let runtime = crate::settings::LocaleRuntime::new(
            locale.clone(),
            events.clone(),
            Arc::new(move || Ok(inventory.registry().supported_languages())),
            Arc::new(du_platform_os::windows_time_zone::WindowsTimeZone),
        );
        #[cfg(target_os = "linux")]
        let runtime = crate::settings::LocaleRuntime::new(
            locale.clone(),
            events.clone(),
            Arc::new(move || Ok(inventory.registry().supported_languages())),
            Arc::new(du_platform_os::linux_time_zone::LinuxTimeZone),
        );
        settings
            .clone()
            .register_locale_profile(&mut builder, runtime, true);
        crate::locale_delivery::register(&mut builder, locale);
    }
    #[cfg(target_os = "macos")]
    {
        let inventory = locale.clone();
        let runtime = crate::settings::LocaleRuntime::read_only(
            locale.clone(),
            Arc::new(move || Ok(inventory.registry().supported_languages())),
            Arc::new(du_platform_os::macos_time_zone::MacOsTimeZone),
        );
        settings
            .clone()
            .register_locale_profile_read_only(&mut builder, runtime);
        crate::locale_delivery::register(&mut builder, locale.clone());
    }
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    analysis::recover_history(
        analysis_registered,
        &events,
        &database,
        capabilities.as_ref(),
    )?;
    #[cfg(windows)]
    analysis::register_history_reads(&mut builder, capabilities.as_ref(), history_reader)?;
    let engine = Arc::new(builder.build().map_err(|error| {
        crate::startup_checks::report_pre_window_error(
            capabilities.as_ref(),
            std::io::Error::other(format!("startup.registration_failed: {error}")),
        )
    })?);
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    let database_owner = database;
    let engine_owner = Arc::new(Mutex::new(Some(engine)));
    let database_owner = Arc::new(Mutex::new(Some(database_owner)));
    #[cfg(windows)]
    let recent_worker_owner = Arc::new(Mutex::new(recent_worker));
    #[cfg(windows)]
    let release_extra_owners = {
        let owner = recent_worker_owner.clone();
        move || {
            if let Ok(mut worker) = owner.lock() {
                worker.take();
            }
        }
    };
    #[cfg(not(windows))]
    let release_extra_owners = || {};
    // Settings keeps a database reference during startup and service registration.
    // The shutdown database owner must be the final app-host reference.
    drop(settings);
    let shutdown = Arc::new(crate::shutdown::host::ShutdownController::new(
        crate::shutdown::host::HostShutdownPorts::new(
            crate::shutdown::host::HostShutdownResources {
                engine: engine_owner.clone(),
                gateway: crate::shutdown::host::GatewayResources {
                    gate: du_mcp_gateway::access::AccessGate::default(),
                    listener: Arc::new(crate::shutdown::host::NoListenerCloser),
                },
                events: events.clone(),
                storage: crate::shutdown::host::StorageResources {
                    database: database_owner,
                    release_extra_owners: Box::new(release_extra_owners),
                },
            },
        ),
    ));
    if let Err(error) = shutdown.announce_running() {
        eprintln!("DiskUnfold shutdown: the running event was not published: {error}");
    }
    let dispatch_engine = engine_owner.clone();
    let dispatch_access = startup_access.clone();
    let dispatch_shutdown = shutdown.clone();
    #[cfg(target_os = "macos")]
    let native_language = locale.language().to_owned();
    let app = WebApp {
        asset: assets::asset,
        dispatch: Arc::new(move |message| {
            crate::ipc::dispatch_with_shutdown(
                dispatch_engine.as_ref(),
                message,
                &dispatch_access,
                dispatch_shutdown.as_ref(),
            )
        }),
        #[cfg(windows)]
        route: crate::ipc::dispatch_lane,
        #[cfg(windows)]
        response_delivery: recent_worker_owner
            .lock()
            .ok()
            .and_then(|worker| worker.as_ref().map(RecentScopeWorker::observer)),
        #[cfg(windows)]
        initialization_script: recent_worker_owner
            .lock()
            .ok()
            .and_then(|worker| worker.as_ref().map(|_| RECENT_SCOPE_BOOTSTRAP)),
        #[cfg(target_os = "macos")]
        locale_enabled: true,
        #[cfg(target_os = "macos")]
        native_language,
        icon: include_bytes!("../icons/icon.png"),
    };
    #[cfg(any(windows, target_os = "linux"))]
    {
        let event_source = crate::shutdown::webview::event_source(events.clone(), shutdown.clone());
        let result = du_platform_os::webview::run_with_events(app, event_source);
        retire_unreleased_session(&engine_owner);
        #[cfg(windows)]
        if let Ok(mut worker) = recent_worker_owner.lock() {
            worker.take();
        }
        finish(result, &shutdown)
    }
    #[cfg(target_os = "macos")]
    {
        // Window close, Dock and menu quit use the same orderly coordinator as the
        // other platforms; the adapter keeps the window open until it completes.
        let hooks = crate::shutdown::webview::shutdown_callbacks(shutdown.clone());
        let result = du_platform_os::webview::run_with_shutdown(app, hooks);
        retire_unreleased_session(&engine_owner);
        finish(result, &shutdown)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = startup_access;
        du_platform_os::webview::run(app)
    }
}

/// Retires the window session if the orderly shutdown did not release the engine.
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn retire_unreleased_session(engine_owner: &Mutex<Option<Arc<du_engine_api::api::Engine>>>) {
    if let Ok(mut engine) = engine_owner.lock()
        && let Some(engine) = engine.take()
    {
        engine.retire_window_session();
    }
}

/// Turns the shutdown outcome into the process result: a window error wins, and a
/// forced or timed-out shutdown exits nonzero.
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn finish(
    window: Result<(), Box<dyn std::error::Error>>,
    shutdown: &crate::shutdown::host::ShutdownController<crate::shutdown::host::HostShutdownPorts>,
) -> Result<(), Box<dyn std::error::Error>> {
    window?;
    shutdown.exit_result().map_err(Into::into)
}
