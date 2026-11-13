//! The digest that binds the embedded GUI manifest and byte store together.
//!
//! `build.rs` carries a copy of this function because the architecture checker
//! rejects shared module paths and `include!`. The test that verifies the assets
//! embedded in this build fails if the two copies disagree. The digest detects a
//! damaged or mismatched asset set; it is not a signature, because the digest
//! travels inside the same executable.
use sha2::{Digest, Sha256};

/// Separates this digest from any other SHA-256 over the same bytes.
const DOMAIN: &[u8] = b"diskunfold.gui-assets.v1\0";

/// Lowercase hex SHA-256 over the domain tag and the length-prefixed manifest and byte store.
///
/// The manifest is covered so a changed name, type or range is detected as well
/// as a changed byte. Length prefixes keep the boundary between the two parts
/// unambiguous.
pub(crate) fn assets_digest_hex(manifest: &str, assets: &[u8]) -> String {
    let length = |len: usize| u64::try_from(len).unwrap_or(u64::MAX).to_le_bytes();
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(length(manifest.len()));
    hasher.update(manifest.as_bytes());
    hasher.update(length(assets.len()));
    hasher.update(assets);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
