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
    du_platform_os::webview::run(WebApp {
        asset,
        dispatch: crate::ipc::dispatch,
        icon: include_bytes!("../icons/icon.png"),
    })
}
