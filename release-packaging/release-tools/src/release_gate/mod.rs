//! Release gate checks run by the F15 release workflow
//! (FR1.HD1.app-host.DD1, FR2.HD1.app-host.DD1, FR5.HD1.app-host.DD1, NFR1.HD1.app-host.DD1).
//!
//! The checks are pure so the workflow, the local dry run and the unit tests share one
//! implementation. Each failure carries a reason key. The keys below are not registered in a
//! published architecture DLD yet and need design registration.

use crate::release_inventory::{
    Cpu, OsFamily, PackageFormat, ReleaseManifest, SignatureMethod, StagedFile,
};
use std::{collections::BTreeSet, fmt, path::PathBuf};

#[cfg(test)]
mod tests;

/// Reason key: the tag is not a stable `vMAJOR.MINOR.PATCH` version.
pub const TAG_FORMAT_INVALID: &str = "release.tag_format_invalid";
/// Reason key: the tag version differs from the workspace package version.
pub const TAG_VERSION_MISMATCH: &str = "release.tag_version_mismatch";
/// Reason key: the tag commit differs from the checked-out or event commit.
pub const TAG_COMMIT_MISMATCH: &str = "release.tag_commit_mismatch";
/// Reason key: a data format version for the build is unset or invalid.
pub const FORMAT_VERSION_MISSING: &str = "release.format_version_missing";
/// Reason key: the recorded format versions differ from the versions defined in code.
pub const FORMAT_VERSION_MISMATCH: &str = "release.format_version_mismatch";
/// Reason key: platform manifests disagree or cannot be merged.
pub const MANIFEST_INCONSISTENT: &str = "release.manifest_inconsistent";
/// Reason key: a required package is absent from the merged release list.
pub const ARTIFACT_MISSING: &str = "release.artifact_missing";
/// Reason key: a row claims a signature that no signing step produced.
pub const SIGNATURE_UNVERIFIED: &str = "release.signature_unverified";
/// Reason key (warning): a designed package format has no builder yet.
pub const FORMAT_UNAVAILABLE: &str = "release.format_unavailable";

/// A failed gate check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GateFailure {
    /// Stable reason key.
    pub reason_key: &'static str,
    /// Human-readable detail.
    pub detail: String,
}

impl GateFailure {
    fn new(reason_key: &'static str, detail: impl Into<String>) -> Self {
        Self {
            reason_key,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for GateFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.reason_key, self.detail)
    }
}

impl std::error::Error for GateFailure {}

/// A validated release tag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseTag {
    version: String,
}

impl ReleaseTag {
    /// Returns the version without the leading `v`.
    pub fn version(&self) -> &str {
        &self.version
    }
}

fn numeric_component(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

/// Accepts only a stable `vMAJOR.MINOR.PATCH` tag without leading zeros or a prefix such as
/// `refs/tags/`.
pub fn parse_release_tag(tag: &str) -> Result<ReleaseTag, GateFailure> {
    let invalid = || {
        GateFailure::new(
            TAG_FORMAT_INVALID,
            format!("tag {tag:?} is not of the form vMAJOR.MINOR.PATCH"),
        )
    };
    let version = tag.strip_prefix('v').ok_or_else(invalid)?;
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|part| numeric_component(part)) {
        return Err(invalid());
    }
    Ok(ReleaseTag {
        version: version.to_owned(),
    })
}

/// Requires the tag version to equal the workspace package version.
pub fn check_tag_version(tag: &ReleaseTag, workspace_version: &str) -> Result<(), GateFailure> {
    if tag.version == workspace_version {
        Ok(())
    } else {
        Err(GateFailure::new(
            TAG_VERSION_MISMATCH,
            format!(
                "tag version {} differs from workspace version {workspace_version}",
                tag.version
            ),
        ))
    }
}

fn is_full_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Requires the commit the tag points to, the checked-out commit and the event commit to be the
/// same lower-case 40 digit commit.
pub fn check_commits(
    tag_commit: &str,
    checkout_commit: &str,
    event_commit: &str,
) -> Result<(), GateFailure> {
    for (name, value) in [
        ("tag commit", tag_commit),
        ("checked-out commit", checkout_commit),
        ("event commit", event_commit),
    ] {
        if !is_full_commit(value) {
            return Err(GateFailure::new(
                TAG_COMMIT_MISMATCH,
                format!("{name} is not a full lower-case commit id"),
            ));
        }
    }
    if tag_commit == checkout_commit && tag_commit == event_commit {
        Ok(())
    } else {
        Err(GateFailure::new(
            TAG_COMMIT_MISMATCH,
            format!(
                "tag commit {tag_commit}, checked-out commit {checkout_commit} and event commit {event_commit} must be identical"
            ),
        ))
    }
}

/// The data format versions embedded into an official build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatVersions {
    /// App database format version (DU_APP_DB_FORMAT).
    pub app_db: u32,
    /// Snapshot file format version (DU_SNAPSHOT_FORMAT).
    pub snapshot: u32,
}

/// Reads `{"appDbFormat": N, "snapshotFormat": N}`. Missing, null, zero or non-integer values fail.
pub fn parse_format_versions(text: &str) -> Result<FormatVersions, GateFailure> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| GateFailure::new(FORMAT_VERSION_MISSING, format!("not JSON: {error}")))?;
    let field = |name: &str| {
        value
            .get(name)
            .and_then(serde_json::Value::as_u64)
            .filter(|number| *number > 0)
            .and_then(|number| u32::try_from(number).ok())
            .ok_or_else(|| {
                GateFailure::new(
                    FORMAT_VERSION_MISSING,
                    format!("{name} must be a positive integer"),
                )
            })
    };
    Ok(FormatVersions {
        app_db: field("appDbFormat")?,
        snapshot: field("snapshotFormat")?,
    })
}

impl FormatVersions {
    /// Returns the format versions the code in this workspace defines: the latest app database
    /// migration number and the DUSNAP envelope version. They come from their owning crates, so
    /// the gate cannot record a number the product does not implement.
    pub const fn from_code() -> Self {
        Self {
            app_db: du_app_db::HISTORY_SCHEMA_VERSION,
            snapshot: du_snapshot_store::envelope::DUSNAP_FORMAT_VERSION as u32,
        }
    }
}

/// Parses the recorded versions and requires them to equal [`FormatVersions::from_code`], so the
/// recorded file cannot drift from the product.
pub fn check_format_versions(text: &str) -> Result<FormatVersions, GateFailure> {
    let recorded = parse_format_versions(text)?;
    let defined = FormatVersions::from_code();
    if recorded == defined {
        Ok(defined)
    } else {
        Err(GateFailure::new(
            FORMAT_VERSION_MISMATCH,
            format!(
                "recorded {recorded:?} but the code defines {defined:?}; update format-versions.json in the release change"
            ),
        ))
    }
}

fn safe_part_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Merges per-platform manifests into one release list. Each part's staged paths are moved under
/// a directory named after the part, so one staging root holds every platform.
pub fn merge_manifests(
    parts: Vec<(String, ReleaseManifest)>,
) -> Result<ReleaseManifest, GateFailure> {
    let inconsistent = |detail: String| GateFailure::new(MANIFEST_INCONSISTENT, detail);
    let Some((_, first)) = parts.first() else {
        return Err(inconsistent("no manifests to merge".into()));
    };
    let mut merged = ReleaseManifest {
        product_name: first.product_name.clone(),
        version: first.version.clone(),
        rows: Vec::new(),
        channel_references: Vec::new(),
        staged_files: Vec::new(),
    };
    let mut seen = BTreeSet::new();
    for (name, part) in parts {
        if !safe_part_name(&name) || !seen.insert(name.clone()) {
            return Err(inconsistent(format!(
                "part name {name:?} is unsafe or repeated"
            )));
        }
        if part.product_name != merged.product_name || part.version != merged.version {
            return Err(inconsistent(format!(
                "{name} describes {} {} but the release is {} {}",
                part.product_name, part.version, merged.product_name, merged.version
            )));
        }
        merged.rows.extend(part.rows);
        merged.channel_references.extend(part.channel_references);
        for file in part.staged_files {
            merged.staged_files.push(StagedFile {
                relative_path: PathBuf::from(&name).join(&file.relative_path),
                ..file
            });
        }
    }
    let mut names = BTreeSet::new();
    for row in &merged.rows {
        if !names.insert(row.file_name.to_ascii_lowercase()) {
            return Err(inconsistent(format!(
                "artifact {} appears in more than one part",
                row.file_name
            )));
        }
    }
    Ok(merged)
}

const REQUIRED: [(OsFamily, Cpu, PackageFormat); 9] = [
    (OsFamily::Windows, Cpu::X64, PackageFormat::PortableExe),
    (OsFamily::Windows, Cpu::Arm64, PackageFormat::PortableExe),
    (OsFamily::Windows, Cpu::X64, PackageFormat::Msix),
    (OsFamily::Windows, Cpu::Arm64, PackageFormat::Msix),
    (OsFamily::Ubuntu, Cpu::X64, PackageFormat::Deb),
    (OsFamily::Ubuntu, Cpu::Arm64, PackageFormat::Deb),
    (OsFamily::Ubuntu, Cpu::X64, PackageFormat::TarGz),
    (OsFamily::Ubuntu, Cpu::Arm64, PackageFormat::TarGz),
    (OsFamily::Macos, Cpu::Universal, PackageFormat::Dmg),
];

/// Requires every package this workflow can build. On success returns warnings for designed
/// formats with no builder yet (MSI); the release notes must state them.
pub fn check_required_artifacts(
    manifest: &ReleaseManifest,
) -> Result<Vec<GateFailure>, GateFailure> {
    check_required_artifacts_scoped(manifest, None)
}

/// Like [`check_required_artifacts`], limited to the given OS and CPU pairs. The local dry run
/// uses this to check only what the current machine can build; the workflow passes `None`.
pub fn check_required_artifacts_scoped(
    manifest: &ReleaseManifest,
    scope: Option<&[(OsFamily, Cpu)]>,
) -> Result<Vec<GateFailure>, GateFailure> {
    let missing: Vec<String> = REQUIRED
        .iter()
        .filter(|(family, cpu, _)| scope.is_none_or(|pairs| pairs.contains(&(*family, *cpu))))
        .filter(|(family, cpu, format)| {
            !manifest
                .rows
                .iter()
                .any(|row| row.os.family == *family && row.cpu == *cpu && row.format == *format)
        })
        .map(|(family, cpu, format)| format!("{family:?}/{cpu:?}/{format:?}"))
        .collect();
    if !missing.is_empty() {
        return Err(GateFailure::new(
            ARTIFACT_MISSING,
            format!("missing packages: {}", missing.join(", ")),
        ));
    }
    let has_msi = manifest
        .rows
        .iter()
        .any(|row| row.format == PackageFormat::Msi);
    Ok(if has_msi {
        Vec::new()
    } else {
        vec![GateFailure::new(
            FORMAT_UNAVAILABLE,
            "no MSI package: the Windows MSI builder is not available yet",
        )]
    })
}

/// When no signing step ran, a row may claim only `unsigned` or, on a macOS row, the ad-hoc or
/// self-signed signature that the macOS packager applies itself.
pub fn check_signature_claims(
    manifest: &ReleaseManifest,
    signing_applied: bool,
) -> Result<(), GateFailure> {
    if signing_applied {
        return Ok(());
    }
    let claimed: Vec<&str> = manifest
        .rows
        .iter()
        .filter(|row| match row.signature {
            SignatureMethod::Unsigned => false,
            SignatureMethod::HardenedRuntimeAdHoc | SignatureMethod::HardenedRuntimeSelfSigned => {
                row.os.family != OsFamily::Macos
            }
            SignatureMethod::SignPath | SignatureMethod::Store => true,
        })
        .map(|row| row.file_name.as_str())
        .collect();
    if claimed.is_empty() {
        Ok(())
    } else {
        Err(GateFailure::new(
            SIGNATURE_UNVERIFIED,
            format!(
                "no signing step ran but these rows claim a signature: {}",
                claimed.join(", ")
            ),
        ))
    }
}
