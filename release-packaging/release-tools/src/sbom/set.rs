//! Checks that every released package has an SBOM that describes it.

use super::{
    ComponentId, check_sbom_coverage, expected::expected_components, mismatch, rust_targets,
    sbom_file_name, sbom_subject_sha256,
};
use crate::{release_gate::GateFailure, release_inventory::ReleaseManifest};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

/// Requires an SBOM for every package row, written for exactly that package file (same SHA-256),
/// covered by THIRD-PARTY-NOTICES and, when `repo` is given, equal to the component set that
/// `cargo tree` and `npm ls` derive for the row's target. Returns the number of SBOMs checked.
///
/// # Errors
/// Returns `release.notice_mismatch` for a missing, unreadable, mis-addressed or disagreeing SBOM,
/// or a row whose OS and CPU have no Rust target.
pub fn check_sbom_set(
    manifest: &ReleaseManifest,
    sbom_dir: &Path,
    notices_text: &str,
    repo: Option<&Path>,
) -> Result<usize, GateFailure> {
    let mut derived: BTreeMap<String, BTreeSet<ComponentId>> = BTreeMap::new();
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
        let expected = match repo {
            None => None,
            Some(repo) => {
                let key = format!("{:?}/{:?}", row.os.family, row.cpu);
                if !derived.contains_key(&key) {
                    let targets = rust_targets(row.os.family, row.cpu)
                        .ok_or_else(|| mismatch(format!("{} has no Rust target", row.file_name)))?;
                    derived.insert(
                        key.clone(),
                        expected_components(repo, targets).map_err(mismatch)?,
                    );
                }
                derived.get(&key)
            }
        };
        check_sbom_coverage(&text, notices_text, expected)
            .map_err(|failure| mismatch(format!("{name}: {}", failure.detail)))?;
    }
    Ok(manifest.rows.len())
}
