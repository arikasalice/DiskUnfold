//! Embedded web assets produced by the build script.
use du_platform_os::webview::Asset;

pub(super) static MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/assets.txt"));
pub(super) static ASSETS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/assets.bin"));
pub(super) static ASSETS_SHA256: &str = include_str!(concat!(env!("OUT_DIR"), "/assets.sha256"));

/// Looks up one embedded asset by its manifest path.
pub(super) fn asset(path: &str) -> Option<Asset> {
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
