//! Checks that every released package has an SBOM that describes it.

use super::{check_sbom_matches_notices, mismatch, sbom_file_name, sbom_subject_sha256};
use crate::{release_gate::GateFailure, release_inventory::ReleaseManifest};
use std::{fs, path::Path};

/// Requires an SBOM for every package row, written for exactly that package file (same SHA-256),
/// and matching THIRD-PARTY-NOTICES. Returns the number of SBOMs checked.
///
/// # Errors
/// Returns `release.notice_mismatch` for a missing, unreadable, mis-addressed or disagreeing SBOM.
pub fn check_sbom_set(
    manifest: &ReleaseManifest,
    sbom_dir: &Path,
    notices_text: &str,
) -> Result<usize, GateFailure> {
    for row in &manifest.rows {
        let name = sbom_file_name(&row.file_name);
        let text = fs::read_to_string(sbom_dir.join(&name)).map_err(|error| {
            mismatch(format!(
                "SBOM {name} for {} cannot be read: {error}",
                row.file_name
            ))
        })?;
        let recorded = sbom_subject_sha256(&text)?;
        if recorded != row.sha256.as_str() {
            return Err(mismatch(format!(
                "SBOM {name} describes SHA-256 {recorded} but {} is {}",
                row.file_name, row.sha256
            )));
        }
        check_sbom_matches_notices(&text, notices_text)
            .map_err(|failure| mismatch(format!("{name}: {}", failure.detail)))?;
    }
    Ok(manifest.rows.len())
}
