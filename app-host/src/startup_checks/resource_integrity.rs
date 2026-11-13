//! Startup row 1: integrity of the GUI assets embedded in the executable.
use std::collections::HashSet;

use du_core_contracts::startup_ipc::{StartupCheck, StartupCheckOutcome, StartupCheckResult};
use sha2::{Digest, Sha256};

/// Reason key for a damaged or inconsistent embedded asset set.
pub(crate) const RESOURCE_INTEGRITY_FAILED: &str = "startup.resource_integrity_failed";

/// The asset every webview session must be able to load.
const ENTRY_ASSET: &str = "/index.html";

/// Verifies the embedded asset manifest, byte store and build-time digest agree.
///
/// `manifest` holds one `name<TAB>mime<TAB>start<TAB>end` line per asset in byte
/// order, `assets` is the concatenated byte store, and `expected_sha256` is the
/// digest the build recorded for that store. Any disagreement is an integrity
/// failure that must stop startup, because the window would serve damaged assets.
pub(crate) fn resource_integrity_observation(
    manifest: &str,
    assets: &[u8],
    expected_sha256: &str,
) -> Result<StartupCheckResult, std::io::Error> {
    if !manifest_is_consistent(manifest, assets.len()) || !digest_matches(assets, expected_sha256) {
        return Err(std::io::Error::other(RESOURCE_INTEGRITY_FAILED));
    }
    Ok(StartupCheckResult {
        check: StartupCheck::ResourceIntegrity,
        result: StartupCheckOutcome::Pass,
        reason_key: None,
    })
}

fn digest_matches(assets: &[u8], expected: &str) -> bool {
    let actual: String = Sha256::digest(assets)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let expected = expected.trim();
    expected.len() == 64 && actual.eq_ignore_ascii_case(expected)
}

/// Entries must tile the byte store exactly, with unique names and the entry page.
fn manifest_is_consistent(manifest: &str, store_len: usize) -> bool {
    let mut next = 0_usize;
    let mut names = HashSet::new();
    let mut has_entry = false;
    for line in manifest.lines() {
        let mut fields = line.split('\t');
        let (Some(name), Some(mime), Some(start), Some(end), None) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            return false;
        };
        let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) else {
            return false;
        };
        if !name.starts_with('/') || mime.is_empty() || start != next || end < start {
            return false;
        }
        if end > store_len || !names.insert(name) {
            return false;
        }
        has_entry |= name == ENTRY_ASSET;
        next = end;
    }
    has_entry && next == store_len
}
