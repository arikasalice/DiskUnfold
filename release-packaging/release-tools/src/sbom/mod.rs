//! CycloneDX SBOM generation and the notices/SBOM comparison (F15 release stage 5, 고지·SBOM 대조;
//! FR2.HD1.app-host.DD1, FR5.HD1.app-host.DD1).
//!
//! An SBOM describes one released package for its target platform: the Cargo crates that a build
//! for the package's Rust target links (`cargo metadata --filter-platform`) and the npm packages
//! bundled into the embedded web assets. THIRD-PARTY-NOTICES covers every target, so it must list
//! at least everything an SBOM lists. The expected set is also derived from `cargo tree` and
//! `npm ls`, a source independent of the inventory that wrote both documents (see
//! [`expected`]); a package missing from the SBOM or the notices fails the release with
//! `release.notice_mismatch`. Build tools appear with scope `excluded` and are never compared.

use crate::{
    notices::{Ecosystem, NoticePackage},
    release_gate::GateFailure,
    release_inventory::{BundledComponent, Cpu, OsFamily},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub mod expected;
mod identity;
mod set;
#[cfg(test)]
mod tests;

pub use identity::sbom_bundled;
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
    /// Rust target triples the package is built for; empty when the SBOM lists every target.
    pub rust_targets: &'a [&'a str],
    /// Third-party files the package carries unmodified (the WebView2 installer in an MSI). They
    /// are listed with their SHA-256 and version and are not part of the notices comparison.
    pub bundled: &'a [BundledComponent],
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

/// Rust target triples that the package for an OS family and CPU is built for. A macOS universal
/// package holds both slices.
pub fn rust_targets(family: OsFamily, cpu: Cpu) -> Option<&'static [&'static str]> {
    match (family, cpu) {
        (OsFamily::Windows, Cpu::X64) => Some(&["x86_64-pc-windows-msvc"]),
        (OsFamily::Windows, Cpu::Arm64) => Some(&["aarch64-pc-windows-msvc"]),
        (OsFamily::Ubuntu, Cpu::X64) => Some(&["x86_64-unknown-linux-gnu"]),
        (OsFamily::Ubuntu, Cpu::Arm64) => Some(&["aarch64-unknown-linux-gnu"]),
        (OsFamily::Macos, Cpu::Universal) => Some(&["aarch64-apple-darwin", "x86_64-apple-darwin"]),
        _ => None,
    }
}

pub(crate) fn mismatch(detail: impl Into<String>) -> GateFailure {
    GateFailure::new(NOTICE_MISMATCH, detail)
}

/// Percent-encodes a purl name or version: everything except unreserved characters (and `/` in a
/// name, which separates an npm scope) becomes `%XX`, so `+` in build metadata is `%2B`.
fn purl_encode(text: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        let plain = byte.is_ascii_alphanumeric()
            || matches!(byte, b'.' | b'-' | b'_' | b'~')
            || (keep_slash && byte == b'/');
        if plain {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn purl(ecosystem: Ecosystem, name: &str, version: &str) -> String {
    let kind = match ecosystem {
        Ecosystem::Cargo => "cargo",
        Ecosystem::Npm => "npm",
    };
    format!(
        "pkg:{kind}/{}@{}",
        purl_encode(name, true),
        purl_encode(version, false)
    )
}

fn license_entry(expression: &str) -> Value {
    if spdx::Expression::parse(expression).is_ok() {
        json!({ "expression": expression })
    } else {
        json!({ "license": { "name": expression } })
    }
}

/// Ecosystem label of a bundled file in the SBOM; such components are compared with the manifest
/// row, not with THIRD-PARTY-NOTICES or the cargo/npm sets.
pub const BUNDLED_ECOSYSTEM: &str = "bundled";

fn bundled_component(component: &BundledComponent) -> Value {
    let reference = format!("bundled:{}@{}", component.name, component.version);
    json!({
        "type": "file",
        "bom-ref": reference,
        "name": component.name,
        "version": component.version,
        "scope": "required",
        "hashes": [{ "alg": "SHA-256", "content": component.sha256.as_str() }],
        "externalReferences": [{ "type": "distribution", "url": component.source }],
        "properties": [
            { "name": "du:ecosystem", "value": BUNDLED_ECOSYSTEM },
            { "name": "du:signer", "value": component.signer }
        ]
    })
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
    components.extend(subject.bundled.iter().map(bundled_component));
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
        "{}\n{}\n{}\n{}",
        subject.sha256,
        subject.rust_targets.join(","),
        build_tools
            .iter()
            .map(|tool| format!("{}={}", tool.name, tool.version))
            .collect::<Vec<_>>()
            .join(","),
        depends_on
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let targets = if subject.rust_targets.is_empty() {
        "all".to_owned()
    } else {
        subject.rust_targets.join(",")
    };
    let subject_properties = vec![
        json!({ "name": "du:file-name", "value": subject.file_name }),
        json!({ "name": "du:rust-targets", "value": targets }),
    ];
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
                "properties": subject_properties
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

/// Checks an SBOM against THIRD-PARTY-NOTICES and, when given, against the expected component
/// set derived independently for the package's target (see [`expected`]).
///
/// - Every required SBOM component must be listed in THIRD-PARTY-NOTICES, which covers all
///   targets and so may list more.
/// - With `expected`, the SBOM must list exactly that set, and the notices must list all of it.
///
/// # Errors
/// Returns `release.notice_mismatch` naming the entries that disagree, or when a document cannot
/// be read.
pub fn check_sbom_coverage(
    sbom_text: &str,
    notices_text: &str,
    expected: Option<&BTreeSet<ComponentId>>,
) -> Result<(), GateFailure> {
    let in_sbom = sbom_components(sbom_text)?;
    let in_notices = notice_identities(notices_text)?;
    let mut parts = Vec::new();
    let mut note = |label: &str, ids: BTreeSet<ComponentId>| {
        if !ids.is_empty() {
            parts.push(describe(label, &ids));
        }
    };
    note(
        "only in the SBOM, not in THIRD-PARTY-NOTICES",
        in_sbom.difference(&in_notices).cloned().collect(),
    );
    if let Some(expected) = expected {
        note(
            "expected for this target but missing from the SBOM",
            expected.difference(&in_sbom).cloned().collect(),
        );
        note(
            "in the SBOM but not expected for this target",
            in_sbom.difference(expected).cloned().collect(),
        );
        note(
            "expected for this target but missing from THIRD-PARTY-NOTICES",
            expected.difference(&in_notices).cloned().collect(),
        );
    }
    if parts.is_empty() {
        Ok(())
    } else {
        Err(mismatch(parts.join("; ")))
    }
}
