//! Header and digest check for the built-in sample bundle, format version 1.
//!
//! The byte layout is owned by `scan-engine/src/sample_bundle/format_v1.rs` (magic `DUSAMPLE`,
//! `u16` little-endian format version, `u32` little-endian header length, header JSON, payload
//! JSON). This module repeats only the header and digest check so release tooling does not depend
//! on the engine crate. A damaged header or a digest that does not match the payload fails.
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

const MAGIC: &[u8; 8] = b"DUSAMPLE";
const PREAMBLE_LEN: usize = 14;
const MAX_HEADER_LEN: usize = 64 * 1024;
const SUPPORTED_FORMAT: u16 = 1;

/// The bundle version and digest recorded in the build provenance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SampleBundleRecord {
    /// Bundle version from the verified header.
    pub bundle_version: String,
    /// Format version of the byte layout.
    pub format_version: u16,
    /// Number of items the header declares.
    pub declared_item_count: u64,
    /// Lowercase hexadecimal BLAKE3 digest of the payload, confirmed against the payload bytes.
    pub blake3: String,
}

/// Why a bundle failed verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleBundleError {
    /// The magic bytes or the preamble are missing.
    NotABundle,
    /// The format version is not one this tool can read.
    UnsupportedFormat(u16),
    /// The header length is zero, too large or beyond the end of the file.
    HeaderLength,
    /// The header is not the expected JSON object.
    HeaderJson,
    /// The digest algorithm is not `blake3` or the digest is not 64 lowercase hex characters.
    DigestField,
    /// The digest does not match the payload.
    DigestMismatch,
}

impl fmt::Display for SampleBundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotABundle => f.write_str("not a sample bundle"),
            Self::UnsupportedFormat(version) => {
                write!(f, "unsupported sample bundle format version {version}")
            }
            Self::HeaderLength => f.write_str("sample bundle header length is invalid"),
            Self::HeaderJson => f.write_str("sample bundle header is damaged"),
            Self::DigestField => f.write_str("sample bundle digest field is invalid"),
            Self::DigestMismatch => f.write_str("sample bundle digest does not match its payload"),
        }
    }
}

impl Error for SampleBundleError {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Header {
    bundle_version: String,
    digest_algorithm: String,
    digest: String,
    declared_item_count: u64,
    #[allow(dead_code)]
    scenario_ids: Vec<String>,
}

/// Verifies the header of a format version 1 bundle and its payload digest.
pub fn verify_sample_bundle(bytes: &[u8]) -> Result<SampleBundleRecord, SampleBundleError> {
    let preamble = bytes
        .get(..PREAMBLE_LEN)
        .ok_or(SampleBundleError::NotABundle)?;
    if &preamble[..8] != MAGIC {
        return Err(SampleBundleError::NotABundle);
    }
    let format_version = u16::from_le_bytes([preamble[8], preamble[9]]);
    if format_version != SUPPORTED_FORMAT {
        return Err(SampleBundleError::UnsupportedFormat(format_version));
    }
    let header_len = u32::from_le_bytes([preamble[10], preamble[11], preamble[12], preamble[13]]);
    let header_len = usize::try_from(header_len).map_err(|_| SampleBundleError::HeaderLength)?;
    let payload_start = PREAMBLE_LEN + header_len;
    if header_len == 0 || header_len > MAX_HEADER_LEN || payload_start > bytes.len() {
        return Err(SampleBundleError::HeaderLength);
    }
    let header: Header = serde_json::from_slice(&bytes[PREAMBLE_LEN..payload_start])
        .map_err(|_| SampleBundleError::HeaderJson)?;
    let valid_digest = header.digest.len() == 64
        && header
            .digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if header.digest_algorithm != "blake3" || !valid_digest {
        return Err(SampleBundleError::DigestField);
    }
    let actual = blake3::hash(&bytes[payload_start..]).to_hex().to_string();
    if actual != header.digest {
        return Err(SampleBundleError::DigestMismatch);
    }
    Ok(SampleBundleRecord {
        bundle_version: header.bundle_version,
        format_version,
        declared_item_count: header.declared_item_count,
        blake3: actual,
    })
}

#[cfg(test)]
mod tests;
