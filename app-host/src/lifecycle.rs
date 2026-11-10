use std::sync::Arc;

use du_engine_api::api::EngineBuilder;
use du_platform_os::webview::{Asset, WebApp};

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
    let events = du_engine_api::desktop::register_with_shared_table_limit(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(history),
        &du_platform_os::entropy::OsRandom,
        settings.scan_limit_source(),
    )?;
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
        isolate: crate::ipc::isolates_volume_enumeration,
        icon: include_bytes!("../icons/icon.png"),
    };
    #[cfg(windows)]
    {
        let result =
            du_platform_os::webview::run_with_events(app, Arc::new(move || events.next_json()));
        engine.retire_window_session();
        result
    }
    #[cfg(not(windows))]
    du_platform_os::webview::run(app)
}
