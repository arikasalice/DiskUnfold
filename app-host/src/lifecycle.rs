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
    #[allow(unused_mut)]
    let mut builder = EngineBuilder::default();
    #[cfg(windows)]
    let events = du_engine_api::desktop::register(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(DeferredHistory),
    )?;
    let engine = builder.build()?;
    let app = WebApp {
        asset,
        dispatch: Arc::new(move |message| crate::ipc::dispatch(&engine, message)),
        #[cfg(windows)]
        isolate: crate::ipc::isolates_volume_enumeration,
        icon: include_bytes!("../icons/icon.png"),
    };
    #[cfg(windows)]
    return du_platform_os::webview::run_with_events(app, Arc::new(move || events.next_json()));
    #[cfg(not(windows))]
    du_platform_os::webview::run(app)
}

/// Until the history repository is composed, report storage failure explicitly.
#[cfg(windows)]
struct DeferredHistory;

#[cfg(windows)]
impl du_core_contracts::job::JobHistory for DeferredHistory {
    fn append(
        &self,
        _: &du_core_contracts::job::JobSummary,
    ) -> Result<du_core_contracts::job::HistoryAppend, String> {
        Err("history.write_deferred".into())
    }
}
