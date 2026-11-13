use super::os_range::OsRange;
use serde::{Deserialize, Serialize, de};
use std::{error::Error, fmt, io, path::PathBuf};

/// A manifest row describing one distribution artifact for one platform and channel.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionRow {
    /// The downloadable package filename. It must identify the product, version, and CPU.
    pub file_name: String,
    /// Product version represented by this artifact.
    pub version: String,
    /// Supported OS family and range or exact supported versions for this artifact.
    pub os: OsRange,
    /// CPU architecture of the artifact.
    pub cpu: Cpu,
    /// Packaging format used for this artifact.
    pub format: PackageFormat,
    /// Primary distribution channel for the artifact.
    pub channel: DistributionChannel,
    /// Signing method recorded for the artifact.
    pub signature: SignatureMethod,
    /// Human-readable scope of the signature method.
    pub signature_scope: String,
    /// SHA-256 of the complete package file.
    pub sha256: Sha256Digest,
    /// Filename expected for the executable associated with this package row.
    ///
    /// Staging verification does not prove that this file is a member of the package.
    pub executable_file_name: String,
    /// SHA-256 declared for the executable associated with this package row.
    ///
    /// The inventory generator rehashes the separately staged executable path, but it does not
    /// inspect the package or prove that this executable is a member of the package.
    pub executable_sha256: Sha256Digest,
    /// User-facing installation and launch instructions.
    pub run_instructions: String,
    /// File name of the CycloneDX SBOM that describes this package.
    ///
    /// Packagers leave it unset; `prepare_inventory` sets it to `<file_name>.cdx.json` for every
    /// row, so the published inventory and `DISTRIBUTION.md` name the SBOM next to the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sbom_file_name: Option<String>,
    /// Third-party components carried inside the package. Empty for packages that carry none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bundled_components: Vec<BundledComponent>,
}

/// A third-party component that a package carries unmodified, such as the WebView2 Evergreen
/// Standalone Installer inside the Windows MSI. The packager records what it verified; the
/// digest is not pinned, because the vendor replaces the file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BundledComponent {
    /// Component name as published by its vendor.
    pub name: String,
    /// Component version read from the bundled file.
    pub version: String,
    /// SHA-256 of the bundled file, as shipped.
    pub sha256: Sha256Digest,
    /// Verified Authenticode signer subject of the bundled file.
    pub signer: String,
    /// Where the file was obtained.
    pub source: String,
}

/// The release list input consumed by the inventory generator.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    /// Product name used to check artifact filenames.
    pub product_name: String,
    /// Release version; every row and channel reference must use this version.
    pub version: String,
    /// Distribution rows from the single release-list source.
    pub rows: Vec<DistributionRow>,
    /// Store and winget entries that reference a distribution row by filename, version, and CPU.
    #[serde(default)]
    pub channel_references: Vec<ChannelReference>,
    /// Local staged files to rehash before emitting public-facing metadata.
    #[serde(default)]
    pub staged_files: Vec<StagedFile>,
}

/// A stable output view that does not disclose local staging paths.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReleaseInventory {
    /// Product name represented by this inventory.
    pub(crate) product_name: String,
    /// Release version represented by this inventory.
    pub(crate) version: String,
    /// Canonically ordered distribution rows.
    pub(crate) rows: Vec<DistributionRow>,
    /// Canonically ordered Store and winget references.
    pub(crate) channel_references: Vec<ChannelReference>,
    /// Outcome of the fuzz gate (see `release_gate::FuzzGateRecord`), recorded so a release with no
    /// fuzz evidence says so. Absent when the inventory was built without a fuzz record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fuzz_gate: Option<serde_json::Value>,
}

/// An explicit channel listing that points to an artifact row in the release list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelReference {
    /// Additional distribution channel that references a release-list row.
    pub channel: UpdateChannel,
    /// Version advertised by the channel entry.
    pub version: String,
    /// CPU advertised by the channel entry.
    pub cpu: Cpu,
    /// Exact artifact row filename referenced by the channel entry.
    pub artifact_file_name: String,
    /// How users receive updates through this channel.
    pub update_method: UpdateMethod,
}

/// A CPU target recorded in the release list.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cpu {
    /// x86-64 / x64.
    X64,
    /// AArch64 / ARM64.
    Arm64,
    /// One universal macOS binary supporting Intel and Apple silicon.
    Universal,
}

impl Cpu {
    pub(super) fn sort_key(self) -> &'static str {
        match self {
            Self::Arm64 => "arm64",
            Self::Universal => "universal",
            Self::X64 => "x64",
        }
    }

    pub(super) fn filename_aliases(self) -> &'static [&'static str] {
        match self {
            Self::Arm64 => &["arm64", "aarch64", "apple-silicon"],
            Self::Universal => &["universal", "universal2"],
            Self::X64 => &["x64", "x86_64", "amd64", "intel"],
        }
    }
}

/// A supported package format.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageFormat {
    /// Windows Installer package.
    Msi,
    /// Portable Windows executable package.
    PortableExe,
    /// Microsoft Store application package.
    Msix,
    /// macOS disk image.
    Dmg,
    /// Debian package.
    Deb,
    /// Gzip-compressed tar archive.
    TarGz,
}

impl PackageFormat {
    pub(super) fn sort_key(self) -> &'static str {
        match self {
            Self::Deb => "deb",
            Self::Dmg => "dmg",
            Self::Msi => "msi",
            Self::Msix => "msix",
            Self::PortableExe => "portable-exe",
            Self::TarGz => "tar.gz",
        }
    }
}

/// The primary release channel for a downloadable artifact.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DistributionChannel {
    /// Direct download from the project's official release page.
    Direct,
    /// Microsoft Store distribution.
    Store,
    /// winget distribution.
    Winget,
}

/// An additional channel entry that points to a row in the release list.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    /// Microsoft Store.
    Store,
    /// winget.
    Winget,
}

/// The signing system used for an artifact.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureMethod {
    /// Windows signing performed by SignPath.
    SignPath,
    /// Store signing for a Store-delivered package.
    Store,
    /// Hardened Runtime ad-hoc signing for a macOS application.
    HardenedRuntimeAdHoc,
    /// No platform signature is applied to the artifact.
    Unsigned,
}

/// The update behavior advertised for a Store or winget channel reference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMethod {
    /// The channel updates the installed package automatically.
    Automatic,
    /// The user runs a channel-specific update command.
    ChannelCommand,
    /// The user downloads and replaces the package directly.
    ReplaceFile,
}

/// The role of a staged file to be reverified.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StagedFileKind {
    /// Complete package file.
    Package,
    /// Executable staged by the packaging job for this row. Its package membership is not checked.
    Executable,
}

/// A local staged file associated with one release-list row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StagedFile {
    /// Artifact row this file belongs to.
    pub artifact_file_name: String,
    /// Whether this path is the package or its packaged executable.
    pub kind: StagedFileKind,
    /// Path relative to the staging root; absolute paths and parent traversal are rejected.
    pub relative_path: PathBuf,
}

/// A digest that is always exactly 32 bytes, displayed as lowercase hexadecimal.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    /// Parses and canonicalizes a 64-character hexadecimal SHA-256 digest.
    pub fn parse(value: &str) -> Result<Self, InventoryError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(InventoryError::InvalidField {
                field: "sha256",
                detail: "must be exactly 64 hexadecimal characters".to_owned(),
            });
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    /// Returns the canonical lowercase hexadecimal digest.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Sha256Digest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

/// A hash verified from one staged package or executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedFile {
    /// Artifact row associated with the staged file.
    pub(crate) artifact_file_name: String,
    /// Basename recorded for this package or executable.
    pub(crate) file_name: String,
    /// Role of this file in the row.
    pub(crate) kind: StagedFileKind,
    /// Verified digest.
    pub(crate) sha256: Sha256Digest,
}

/// A failure while validating release rows, channel references, or staged files.
#[derive(Debug)]
pub enum InventoryError {
    /// A field is missing, malformed, or inconsistent with the release list.
    InvalidField {
        /// Field name associated with the failure.
        field: &'static str,
        /// Human-readable reason.
        detail: String,
    },
    /// An artifact filename appears more than once.
    DuplicateArtifact(String),
    /// A Store or winget reference has no unique matching release row.
    InvalidChannelReference(String),
    /// A staged file entry is missing, duplicated, or unsafe.
    InvalidStagedFile(String),
    /// A staged file's recomputed digest differs from the declared release-list digest.
    DigestMismatch {
        /// Artifact filename associated with the failure.
        artifact_file_name: String,
        /// Role of the mismatched file.
        kind: StagedFileKind,
        /// Digest declared in the release list.
        expected: Sha256Digest,
        /// Digest recomputed from the staged file.
        actual: Sha256Digest,
    },
    /// A filesystem operation failed while reading a staged file.
    Io {
        /// Path involved in the failed operation.
        path: PathBuf,
        /// Original filesystem error.
        source: io::Error,
    },
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField { field, detail } => write!(f, "invalid {field}: {detail}"),
            Self::DuplicateArtifact(name) => write!(f, "duplicate artifact filename: {name}"),
            Self::InvalidChannelReference(detail) => {
                write!(f, "invalid channel reference: {detail}")
            }
            Self::InvalidStagedFile(detail) => write!(f, "invalid staged file: {detail}"),
            Self::DigestMismatch {
                artifact_file_name,
                kind,
                expected,
                actual,
            } => write!(
                f,
                "{kind:?} SHA-256 mismatch for {artifact_file_name}: expected {expected}, got {actual}"
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl Error for InventoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
