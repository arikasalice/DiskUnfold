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
    /// Supported OS family and inclusive version range for this artifact.
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
    pub channel_references: Vec<ChannelReference>,
    /// Local staged files to rehash before emitting public-facing metadata.
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

/// The operating-system family supported by a distribution row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OsFamily {
    /// Microsoft Windows.
    Windows,
    /// Apple macOS.
    Macos,
    /// Ubuntu Linux.
    Ubuntu,
}

impl OsFamily {
    pub(super) fn sort_key(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Ubuntu => "ubuntu",
            Self::Windows => "windows",
        }
    }
}

/// An OS release label ordered by its numeric components.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OsVersion(String);

impl OsVersion {
    /// Parses an OS release label such as `11 25H2`, `15`, or `24.04`.
    pub fn parse(value: &str) -> Result<Self, InventoryError> {
        if value.trim().is_empty() || value.chars().any(char::is_control) {
            return Err(InventoryError::InvalidField {
                field: "os_version",
                detail: "must be nonempty and contain no control characters".to_owned(),
            });
        }
        let numeric_groups = numeric_groups(value)?;
        if numeric_groups.is_empty() {
            return Err(InventoryError::InvalidField {
                field: "os_version",
                detail: "must contain at least one numeric release component".to_owned(),
            });
        }
        Ok(Self(value.trim().to_owned()))
    }
}

impl fmt::Display for OsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Ord for OsVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        numeric_groups(&self.0)
            .unwrap_or_default()
            .cmp(&numeric_groups(&other.0).unwrap_or_default())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for OsVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'de> Deserialize<'de> for OsVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

fn numeric_groups(value: &str) -> Result<Vec<u32>, InventoryError> {
    let mut groups = Vec::new();
    let mut number = 0_u32;
    let mut in_number = false;
    for character in value.chars() {
        if let Some(digit) = character.to_digit(10).filter(|_| character.is_ascii()) {
            number = number
                .checked_mul(10)
                .and_then(|current| current.checked_add(digit))
                .ok_or_else(|| InventoryError::InvalidField {
                    field: "os_version",
                    detail: "numeric release component is too large".to_owned(),
                })?;
            in_number = true;
        } else if in_number {
            groups.push(number);
            number = 0;
            in_number = false;
        }
    }
    if in_number {
        groups.push(number);
    }
    Ok(groups)
}

/// An inclusive supported OS version range. `maximum: None` means no declared upper bound.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OsRange {
    /// OS family for this range.
    pub family: OsFamily,
    /// Lowest supported version, inclusive.
    pub minimum: OsVersion,
    /// Highest supported version, inclusive, when bounded.
    pub maximum: Option<OsVersion>,
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
