use std::sync::Arc;

use du_core_contracts::startup::StartupCapabilitiesPort;
use du_engine_api::api::EngineBuilder;
use du_platform_os::startup::SystemStartupCapabilities;
use du_platform_os::webview::{Asset, WebApp};
#[cfg(any(windows, target_os = "linux"))]
use du_platform_os::webview::{EventCallbacks, Events};

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
    )
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
    #[cfg(any(windows, target_os = "linux"))]
    let mut startup_observed = vec![webview_observation];
    #[cfg(target_os = "macos")]
    let _webview_observation = webview_observation;
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
    let (locale, settings, first_run) = {
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
        (locale, settings, first_run)
    };
    #[allow(unused_mut)]
    let mut builder = EngineBuilder::default();
    #[cfg(any(windows, target_os = "linux"))]
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
    #[cfg(target_os = "macos")]
    crate::startup_checks::register_access(&mut builder, startup_access.clone());
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
        settings.register_locale_profile(&mut builder, runtime, true);
        crate::locale_delivery::register(&mut builder, locale);
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
    let dispatch_engine = engine.clone();
    let dispatch_access = startup_access.clone();
    let app = WebApp {
        asset,
        dispatch: Arc::new(move |message| {
            crate::ipc::dispatch_with_startup_access(&dispatch_engine, message, &dispatch_access)
        }),
        #[cfg(windows)]
        route: crate::ipc::dispatch_lane,
        #[cfg(windows)]
        response_delivery: recent_worker.as_ref().map(RecentScopeWorker::observer),
        #[cfg(windows)]
        initialization_script: recent_worker.as_ref().map(|_| RECENT_SCOPE_BOOTSTRAP),
        icon: include_bytes!("../icons/icon.png"),
    };
    #[cfg(any(windows, target_os = "linux"))]
    {
        let event_source = desktop_event_source(events.clone());
        let result = du_platform_os::webview::run_with_events(app, event_source);
        #[cfg(windows)]
        drop(recent_worker);
        engine.retire_window_session();
        result
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = startup_access;
        du_platform_os::webview::run(app)
    }
}

#[cfg(any(windows, target_os = "linux"))]
struct DesktopEventCallbacks {
    next: Box<dyn Fn() -> Option<String> + Send + Sync>,
    post_failed: Box<dyn Fn(du_platform_os::webview::EventPostFailure) + Send + Sync>,
    document_started: Box<dyn Fn() + Send + Sync>,
    document_ready: Box<dyn Fn() + Send + Sync>,
    document_closed: Box<dyn Fn() + Send + Sync>,
}

#[cfg(any(windows, target_os = "linux"))]
impl DesktopEventCallbacks {
    fn new(events: Arc<du_engine_api::desktop::DesktopEvents>) -> Self {
        let next_events = events.clone();
        let failed_events = events.clone();
        let started_events = events.clone();
        let ready_events = events.clone();
        let closed_events = events;
        Self {
            next: Box::new(move || next_events.next_json()),
            post_failed: Box::new(move |failure| {
                failed_events.report_post_failure(failure.failed, failure.discarded);
            }),
            document_started: Box::new(move || started_events.begin_document_generation()),
            document_ready: Box::new(move || ready_events.mark_document_ready()),
            document_closed: Box::new(move || closed_events.close_document_generation()),
        }
    }

    fn into_event_source(self) -> Events {
        Events::new(
            self.next,
            EventCallbacks::new(
                self.post_failed,
                self.document_started,
                self.document_ready,
                self.document_closed,
            ),
        )
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn desktop_event_source(events: Arc<du_engine_api::desktop::DesktopEvents>) -> Events {
    DesktopEventCallbacks::new(events).into_event_source()
}

#[cfg(all(test, windows))]
mod delivery_event_tests;
