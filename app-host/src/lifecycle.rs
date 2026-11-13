use std::sync::{Arc, Mutex};

use du_core_contracts::startup::StartupCapabilitiesPort;
use du_engine_api::api::EngineBuilder;
use du_platform_os::startup::SystemStartupCapabilities;
use du_platform_os::webview::{Asset, WebApp};

#[cfg(windows)]
use crate::recent_scope_worker::{RecentScopeWorker, RecentScopeWriteWarning};
#[cfg(windows)]
use std::num::NonZeroUsize;

static MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/assets.txt"));
static ASSETS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/assets.bin"));

#[cfg(windows)]
const RECENT_SCOPE_BOOTSTRAP: &str = r#"
if (window === window.top && location.href === 'https://diskunfold.invalid/index.html') {
    Object.defineProperty(window, '__diskunfoldRecentScopes', { value: true });
}
"#;

fn asset(path: &str) -> Option<Asset> {
    let mut fields = MANIFEST.lines().find_map(|line| {
        let (name, data) = line.split_once('\t')?;
        (name == path).then_some(data.split('\t'))
    })?;
    let mime = fields.next()?;
    let start: usize = fields.next()?.parse().ok()?;
    let end: usize = fields.next()?.parse().ok()?;
    Some(Asset {
        mime,
        bytes: ASSETS.get(start..end)?,
    })
}

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
    let webview_observation =
        match crate::startup_checks::webview_observation(capabilities.as_ref()) {
            Ok(observation) => observation,
            Err(error) => {
                return Err(Box::new(crate::startup_checks::report_pre_window_error(
                    capabilities.as_ref(),
                    error,
                )));
            }
        };
    let startup_access = Arc::new(crate::startup_checks::StartupAccessSession::new(
        capabilities.clone(),
    ));
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    let mut startup_observed = vec![webview_observation];
    #[cfg(windows)]
    let (locale, settings, history, history_reader, recovery, first_run) = {
        let bundles = crate::locale::register().map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
        let configured = bundles.configured_languages().collect::<Vec<_>>();
        let data_dir = du_platform_os::application_data::directory()
            .ok_or_else(|| crate::startup_database::data_folder_error(None))
            .map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?;
        let opened = crate::startup_database::open_with_status(&data_dir).map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
        let first_run = opened.first_run;
        startup_observed.push(crate::startup_checks::data_format_and_open_observation(
            first_run,
        ));
        let database = opened.database;
        let settings =
            crate::settings::StartupSettings::load(&database, &configured).map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?;
        let stored = settings.stored_language().map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
        let locale = Arc::new(
            crate::locale::initialize(bundles, stored.as_deref()).map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?,
        );
        let reader =
            crate::history::HistoryReader::open(&data_dir, database.clone()).map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?;
        (
            locale,
            settings,
            database.history(crate::settings::current_time_ms),
            reader,
            database,
            first_run,
        )
    };
    #[cfg(target_os = "linux")]
    let (locale, settings, database, first_run) = {
        let bundles = crate::locale::register().map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
        let configured = bundles.configured_languages().collect::<Vec<_>>();
        let data_dir = du_platform_os::application_data::directory()
            .ok_or_else(|| crate::startup_database::data_folder_error(None))
            .map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?;
        let opened = crate::startup_database::open_with_status(&data_dir).map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
        let first_run = opened.first_run;
        startup_observed.push(crate::startup_checks::data_format_and_open_observation(
            first_run,
        ));
        let database = opened.database;
        let settings =
            crate::settings::StartupSettings::load(&database, &configured).map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?;
        let stored = settings.stored_language().map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
        let locale = Arc::new(
            crate::locale::initialize(bundles, stored.as_deref()).map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?,
        );
        (locale, settings, database, first_run)
    };
    #[cfg(target_os = "macos")]
    let (database, settings, locale, first_run) = {
        let bundles = crate::locale::register().map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
        let configured = bundles.configured_languages().collect::<Vec<_>>();
        let data_dir = du_platform_os::application_data::directory()
            .ok_or_else(|| crate::startup_database::data_folder_error(None))
            .map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?;
        let opened = crate::startup_database::open_with_status(&data_dir).map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
        let first_run = opened.first_run;
        startup_observed.push(crate::startup_checks::data_format_and_open_observation(
            first_run,
        ));
        let database = opened.database;
        let settings =
            crate::settings::StartupSettings::load(&database, &configured).map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?;
        let stored = settings.stored_language().map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
        let locale = Arc::new(
            crate::locale::initialize(bundles, stored.as_deref()).map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?,
        );
        (database, settings, locale, first_run)
    };
    #[allow(unused_mut)]
    let mut builder = EngineBuilder::default();
    crate::product_ipc::register(
        &mut builder,
        crate::product_ipc::ProductServices::system(product),
    );
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    {
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
    #[cfg(windows)]
    let desktop = {
        use du_engine_api::desktop;
        let roots = Arc::new(du_platform_os::windows_observation::WindowsScopeRoots);
        let volumes = Arc::new(du_platform_os::volume::WindowsVolumes);
        let history = Arc::new(history);
        let random = &du_platform_os::entropy::OsRandom;
        let scan_limit = settings.clone().scan_limit_source();
        if recent.is_some() {
            crate::recent_scope_open::register(
                &mut builder,
                recovery.clone(),
                roots.clone(),
                volumes.clone(),
            );
            // Bound retained handles independently from the number of persisted rows.
            const PENDING_ADMISSIONS: NonZeroUsize = NonZeroUsize::new(32).unwrap();
            desktop::register_services_with_recent_scope_worker(
                &mut builder,
                roots,
                volumes,
                history,
                random,
                scan_limit,
                PENDING_ADMISSIONS,
            )
            .map_err(|error| {
                crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
            })?
        } else {
            desktop::register_services(&mut builder, roots, volumes, history, random, scan_limit)
                .map_err(|error| {
                    crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
                })?
        }
    };
    #[cfg(windows)]
    let recent_worker = recent
        .map(|options| {
            let admissions = desktop
                .recent_scope_admissions
                .ok_or_else(|| std::io::Error::other("recent scope admission queue unavailable"))?;
            crate::recent_scope_list::register(&mut builder, recovery.clone(), options.cap);
            RecentScopeWorker::start(admissions, recovery.clone(), options.cap, options.warning)
        })
        .transpose()
        .map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
    #[cfg(windows)]
    let events = {
        use du_platform_os::windows_clipboard::WindowsClipboard;
        builder.register(crate::clipboard::CopyPath::new(
            desktop.path_copy,
            WindowsClipboard,
            |path: &du_core_contracts::result::ResultNativePath| WindowsClipboard.path_text(path),
            "\r\n",
        ));
        desktop.events
    };
    #[cfg(target_os = "linux")]
    let events = Arc::new(du_engine_api::desktop::DesktopEvents::default());
    #[cfg(target_os = "macos")]
    let events = Arc::new(du_engine_api::desktop::DesktopEvents::default());
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
    #[cfg(windows)]
    events
        .recover_history(&recovery, crate::settings::current_time_ms())
        .map_err(std::io::Error::other)
        .map_err(|error| {
            crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
        })?;
    #[cfg(windows)]
    du_engine_api::history::detail::register(&mut builder, history_reader.clone());
    #[cfg(windows)]
    du_engine_api::history::items::register(
        &mut builder,
        history_reader.clone(),
        &du_platform_os::entropy::OsRandom,
    )
    .map_err(|error| std::io::Error::other(format!("history.storage_unavailable: {error:?}")))
    .map_err(|error| {
        crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
    })?;
    #[cfg(windows)]
    du_engine_api::history::register(
        &mut builder,
        history_reader,
        &du_platform_os::entropy::OsRandom,
    )
    .map_err(|error| std::io::Error::other(format!("history.storage_unavailable: {error:?}")))
    .map_err(|error| {
        crate::startup_checks::report_pre_window_error(capabilities.as_ref(), error)
    })?;
    let engine = Arc::new(builder.build().map_err(|error| {
        crate::startup_checks::report_pre_window_error(
            capabilities.as_ref(),
            std::io::Error::other(format!("startup.registration_failed: {error}")),
        )
    })?);
    #[cfg(windows)]
    let database_owner = recovery;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
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
    let app = WebApp {
        asset,
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
