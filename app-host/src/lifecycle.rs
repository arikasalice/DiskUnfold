use std::sync::Arc;

use du_engine_api::api::EngineBuilder;
use du_platform_os::webview::{Asset, WebApp};

#[cfg(windows)]
use crate::recent_scope_worker::{RecentScopeWorker, RecentScopeWriteWarning};
#[cfg(windows)]
use std::num::NonZeroUsize;

static MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/assets.txt"));
static ASSETS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/assets.bin"));

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
        let locale = crate::locale::initialize(bundles, stored.as_deref())?;
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
        let scan_limit = settings.scan_limit_source();
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
        dispatch: Arc::new(move |message| {
            #[cfg(windows)]
            let _ = &locale;
            crate::ipc::dispatch(&dispatch_engine, message)
        }),
        #[cfg(windows)]
        route: crate::ipc::dispatch_lane,
        #[cfg(windows)]
        response_delivery: recent_worker.as_ref().map(RecentScopeWorker::observer),
        icon: include_bytes!("../icons/icon.png"),
    };
    #[cfg(windows)]
    {
        let result =
            du_platform_os::webview::run_with_events(app, Arc::new(move || events.next_json()));
        drop(recent_worker);
        engine.retire_window_session();
        result
    }
    #[cfg(not(windows))]
    du_platform_os::webview::run(app)
}
