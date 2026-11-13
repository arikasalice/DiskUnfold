//! Reads component identities back from an SBOM and from THIRD-PARTY-NOTICES.

use super::{ComponentId, mismatch};
use crate::release_gate::GateFailure;
use serde_json::Value;
use std::collections::BTreeSet;

fn parse(sbom_text: &str) -> Result<Value, GateFailure> {
    let value: Value = serde_json::from_str(sbom_text)
        .map_err(|error| mismatch(format!("SBOM is not JSON: {error}")))?;
    if value["bomFormat"] != "CycloneDX" {
        return Err(mismatch("SBOM is not a CycloneDX document"));
    }
    Ok(value)
}

fn ecosystem(component: &Value) -> Option<&str> {
    component["properties"]
        .as_array()?
        .iter()
        .find(|property| property["name"] == "du:ecosystem")
        .and_then(|property| property["value"].as_str())
}

/// Returns the identities of the components with scope `required`.
///
/// # Errors
/// Returns `release.notice_mismatch` when the document is not a CycloneDX SBOM or a required
/// component has no ecosystem, name or version.
pub fn sbom_components(sbom_text: &str) -> Result<BTreeSet<ComponentId>, GateFailure> {
    let value = parse(sbom_text)?;
    let components = value["components"]
        .as_array()
        .ok_or_else(|| mismatch("SBOM has no components list"))?;
    let mut ids = BTreeSet::new();
    for component in components.iter().filter(|c| c["scope"] == "required") {
        let identity = (
            ecosystem(component),
            component["name"].as_str(),
            component["version"].as_str(),
        );
        let (Some(kind), Some(name), Some(version)) = identity else {
            return Err(mismatch(format!(
                "SBOM component {component} lacks an ecosystem, name or version"
            )));
        };
        ids.insert((kind.to_owned(), name.to_owned(), version.to_owned()));
    }
    Ok(ids)
}

/// Returns the SHA-256 recorded for the package the SBOM describes.
///
/// # Errors
/// Returns `release.notice_mismatch` when the document has no subject digest.
pub fn sbom_subject_sha256(sbom_text: &str) -> Result<String, GateFailure> {
    let value = parse(sbom_text)?;
    value["metadata"]["component"]["hashes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|hash| hash["alg"] == "SHA-256")
        .and_then(|hash| hash["content"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| mismatch("SBOM subject has no SHA-256 hash"))
}

/// Reads the package lines (`cargo NAME VERSION: LICENSE`) from the list at the top of
/// THIRD-PARTY-NOTICES. The list ends at the first license text separator.
///
/// # Errors
/// Returns `release.notice_mismatch` when the text lists no package, which means the notices are
/// not the generated document.
pub fn notice_identities(notices_text: &str) -> Result<BTreeSet<ComponentId>, GateFailure> {
    let mut ids = BTreeSet::new();
    for line in notices_text.lines() {
        if line.starts_with("====") {
            break;
        }
        let Some((kind, rest)) = line.split_once(' ') else {
            continue;
        };
        if kind != "cargo" && kind != "npm" {
            continue;
        }
        let Some((name, rest)) = rest.split_once(' ') else {
            continue;
        };
        let Some((version, _license)) = rest.split_once(": ") else {
            continue;
        };
        ids.insert((kind.to_owned(), name.to_owned(), version.to_owned()));
    }
    if ids.is_empty() {
        return Err(mismatch("THIRD-PARTY-NOTICES lists no package"));
    }
    Ok(ids)
}
