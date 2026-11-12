use std::sync::Arc;

use du_engine_api::api::EngineBuilder;
use du_platform_os::webview::{Asset, WebApp};
#[cfg(windows)]
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
    )
}

/// Runs the Windows GUI with an explicitly supplied recent-scope retention policy.
/// The warning sink runs on the background storage thread and receives no path data.
#[cfg(windows)]
pub fn run_with_recent_scope(
    cap: NonZeroUsize,
    warning: impl Fn(RecentScopeWriteWarning) + Send + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    run_app(Some(RecentScopeOptions {
        cap,
        warning: Box::new(warning),
    }))
}

#[cfg(windows)]
struct RecentScopeOptions {
    cap: NonZeroUsize,
    warning: Box<dyn Fn(RecentScopeWriteWarning) + Send>,
}

fn run_app(
    #[cfg(windows)] recent: Option<RecentScopeOptions>,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    let (locale, settings, history, history_reader, recovery) = {
        let bundles = crate::locale::register()?;
        let configured = bundles.configured_languages().collect::<Vec<_>>();
        let data_dir = du_platform_os::application_data::directory().ok_or_else(|| {
            std::io::Error::other("db.open_failed: no Windows application data directory")
        })?;
        let database = crate::startup_database::open(&data_dir)?;
        let settings = crate::settings::StartupSettings::load(&database, &configured)?;
        let stored = settings.stored_language()?;
        let locale = Arc::new(crate::locale::initialize(bundles, stored.as_deref())?);
        let reader = crate::history::HistoryReader::open(&data_dir, database.clone())?;
        (
            locale,
            settings,
            database.history(crate::settings::current_time_ms),
            reader,
            database,
        )
    };
    #[allow(unused_mut)]
    let mut builder = EngineBuilder::default();
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
            )?
        } else {
            desktop::register_services(&mut builder, roots, volumes, history, random, scan_limit)?
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
        .transpose()?;
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
    #[cfg(windows)]
    {
        let inventory = locale.clone();
        let runtime = crate::settings::LocaleRuntime::new(
            locale.clone(),
            events.clone(),
            Arc::new(move || Ok(inventory.registry().supported_languages())),
            Arc::new(du_platform_os::windows_time_zone::WindowsTimeZone),
        );
        settings.register_locale_profile(&mut builder, runtime, true);
        crate::locale_delivery::register(&mut builder, locale);
    }
    #[cfg(windows)]
    events
        .recover_history(&recovery, crate::settings::current_time_ms())
        .map_err(std::io::Error::other)?;
    #[cfg(windows)]
    du_engine_api::history::detail::register(&mut builder, history_reader.clone());
    #[cfg(windows)]
    du_engine_api::history::items::register(
        &mut builder,
        history_reader.clone(),
        &du_platform_os::entropy::OsRandom,
    )
    .map_err(|error| std::io::Error::other(format!("history.storage_unavailable: {error:?}")))?;
    #[cfg(windows)]
    du_engine_api::history::register(
        &mut builder,
        history_reader,
        &du_platform_os::entropy::OsRandom,
    )
    .map_err(|error| std::io::Error::other(format!("history.storage_unavailable: {error:?}")))?;
    let engine = Arc::new(builder.build()?);
    let dispatch_engine = engine.clone();
    let app = WebApp {
        asset,
        dispatch: Arc::new(move |message| crate::ipc::dispatch(&dispatch_engine, message)),
        #[cfg(windows)]
        route: crate::ipc::dispatch_lane,
        #[cfg(windows)]
        response_delivery: recent_worker.as_ref().map(RecentScopeWorker::observer),
        #[cfg(windows)]
        initialization_script: recent_worker.as_ref().map(|_| RECENT_SCOPE_BOOTSTRAP),
        icon: include_bytes!("../icons/icon.png"),
    };
    #[cfg(windows)]
    {
        let event_source = desktop_event_source(events.clone());
        let result = du_platform_os::webview::run_with_events(app, event_source);
        drop(recent_worker);
        engine.retire_window_session();
        result
    }
    #[cfg(not(windows))]
    du_platform_os::webview::run(app)
}

#[cfg(windows)]
struct DesktopEventCallbacks {
    next: Box<dyn Fn() -> Option<String> + Send + Sync>,
    post_failed: Box<dyn Fn(du_platform_os::webview::EventPostFailure) + Send + Sync>,
    document_started: Box<dyn Fn() + Send + Sync>,
    document_ready: Box<dyn Fn() + Send + Sync>,
    document_closed: Box<dyn Fn() + Send + Sync>,
}

#[cfg(windows)]
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

#[cfg(windows)]
fn desktop_event_source(events: Arc<du_engine_api::desktop::DesktopEvents>) -> Events {
    DesktopEventCallbacks::new(events).into_event_source()
}

#[cfg(all(test, windows))]
mod delivery_event_tests;
