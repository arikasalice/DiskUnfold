//! CycloneDX SBOM generation and the notices/SBOM comparison (F15 release stage 5, 고지·SBOM 대조;
//! FR2.HD1.app-host.DD1, FR5.HD1.app-host.DD1).
//!
//! An SBOM describes one released package. It lists the production packages that
//! THIRD-PARTY-NOTICES lists (cargo crates linked into the launcher and npm packages bundled
//! into the embedded web assets), so both documents come from the same inventory and a package
//! that appears in only one of them fails the release with `release.notice_mismatch`. Build
//! tools appear with scope `excluded` and are never compared with the notices.

use crate::{
    notices::{Ecosystem, NoticePackage},
    release_gate::GateFailure,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

mod identity;
mod set;
#[cfg(test)]
mod tests;

pub use identity::{notice_identities, sbom_components, sbom_subject_sha256};
pub use set::check_sbom_set;

/// Reason key: the generated notices and the SBOM, or a package and its SBOM, disagree.
pub const NOTICE_MISMATCH: &str = "release.notice_mismatch";

/// One component identity: ecosystem label (`cargo` or `npm`), name and version.
pub type ComponentId = (String, String, String);

/// The released package an SBOM describes.
#[derive(Clone, Copy, Debug)]
pub struct SbomSubject<'a> {
    /// Product name.
    pub product_name: &'a str,
    /// Release version.
    pub version: &'a str,
    /// File name of the released package.
    pub file_name: &'a str,
    /// SHA-256 of the released package, lower-case hexadecimal.
    pub sha256: &'a str,
}

/// A build-phase tool recorded in the SBOM with scope `excluded`.
#[derive(Clone, Copy, Debug)]
pub struct BuildTool<'a> {
    /// Tool name such as `rustc` or `node`.
    pub name: &'a str,
    /// Pinned tool version.
    pub version: &'a str,
}

/// File name of the SBOM that belongs to a released package.
pub fn sbom_file_name(package_file_name: &str) -> String {
    format!("{package_file_name}.cdx.json")
}

pub(crate) fn mismatch(detail: impl Into<String>) -> GateFailure {
    GateFailure::new(NOTICE_MISMATCH, detail)
}

fn purl(ecosystem: Ecosystem, name: &str, version: &str) -> String {
    match ecosystem {
        Ecosystem::Cargo => format!("pkg:cargo/{name}@{version}"),
        Ecosystem::Npm => format!("pkg:npm/{}@{version}", name.replacen('@', "%40", 1)),
    }
}

fn license_entry(expression: &str) -> Value {
    if spdx::Expression::parse(expression).is_ok() {
        json!({ "expression": expression })
    } else {
        json!({ "license": { "name": expression } })
    }
}

fn component(package: &NoticePackage) -> Value {
    let reference = purl(package.ecosystem, &package.name, &package.version);
    let mut value = json!({
        "type": "library",
        "bom-ref": reference,
        "name": package.name,
        "version": package.version,
        "purl": reference,
        "scope": "required",
        "properties": [{ "name": "du:ecosystem", "value": package.ecosystem.label() }]
    });
    if let Some(license) = &package.license {
        value["licenses"] = json!([license_entry(license)]);
    }
    value
}

/// Derives a stable RFC 4122 style serial number from the SBOM content, so two runs over the same
/// inputs write the same bytes.
fn serial_number(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// Renders the CycloneDX 1.5 JSON document for one released package.
///
/// Packages are sorted and de-duplicated by identity and the output has no timestamp, so the
/// same inputs give the same bytes.
///
/// # Errors
/// Returns the serialization error when the document cannot be written.
pub fn render_cyclonedx(
    subject: &SbomSubject<'_>,
    packages: &[NoticePackage],
    build_tools: &[BuildTool<'_>],
) -> Result<String, serde_json::Error> {
    let mut seen = BTreeSet::new();
    let mut sorted: Vec<&NoticePackage> = packages
        .iter()
        .filter(|package| {
            seen.insert((
                package.ecosystem,
                package.name.clone(),
                package.version.clone(),
            ))
        })
        .collect();
    sorted.sort_by(|a, b| {
        (a.ecosystem, &a.name, &a.version).cmp(&(b.ecosystem, &b.name, &b.version))
    });
    let mut components: Vec<Value> = sorted.iter().map(|package| component(package)).collect();
    let depends_on: Vec<Value> = components.iter().map(|c| c["bom-ref"].clone()).collect();
    for tool in build_tools {
        components.push(json!({
            "type": "application",
            "bom-ref": format!("build:{}", tool.name),
            "name": tool.name,
            "version": tool.version,
            "scope": "excluded",
            "properties": [{ "name": "du:phase", "value": "build" }]
        }));
    }
    let seed = format!(
        "{}\n{}",
        subject.sha256,
        depends_on
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let document = json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": serial_number(&seed),
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "bom-ref": subject.file_name,
                "name": subject.product_name,
                "version": subject.version,
                "description": format!("Released package {}", subject.file_name),
                "hashes": [{ "alg": "SHA-256", "content": subject.sha256 }],
                "properties": [{ "name": "du:file-name", "value": subject.file_name }]
            }
        },
        "components": components,
        "dependencies": [{ "ref": subject.file_name, "dependsOn": depends_on }]
    });
    Ok(serde_json::to_string_pretty(&document)? + "\n")
}

fn describe(label: &str, ids: &BTreeSet<ComponentId>) -> String {
    let shown: Vec<String> = ids
        .iter()
        .take(8)
        .map(|(ecosystem, name, version)| format!("{ecosystem} {name} {version}"))
        .collect();
    let more = ids.len().saturating_sub(shown.len());
    let suffix = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    format!("{label}: {}{suffix}", shown.join(", "))
}

/// Requires the required components of the SBOM and the packages listed in THIRD-PARTY-NOTICES
/// to be the same set.
///
/// # Errors
/// Returns `release.notice_mismatch` naming the entries present on one side only, or when either
/// document cannot be read.
pub fn check_sbom_matches_notices(sbom_text: &str, notices_text: &str) -> Result<(), GateFailure> {
    let in_sbom = sbom_components(sbom_text)?;
    let in_notices = notice_identities(notices_text)?;
    let only_sbom: BTreeSet<ComponentId> = in_sbom.difference(&in_notices).cloned().collect();
    let only_notices: BTreeSet<ComponentId> = in_notices.difference(&in_sbom).cloned().collect();
    if only_sbom.is_empty() && only_notices.is_empty() {
        return Ok(());
    }
    let mut parts = Vec::new();
    if !only_sbom.is_empty() {
        parts.push(describe("only in the SBOM", &only_sbom));
    }
    if !only_notices.is_empty() {
        parts.push(describe("only in THIRD-PARTY-NOTICES", &only_notices));
    }
    Err(mismatch(parts.join("; ")))
}
