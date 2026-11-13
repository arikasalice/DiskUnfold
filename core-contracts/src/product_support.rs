//! Product metadata and observed support values shared by local entry points.
use serde::{Deserialize, Serialize};

/// The kind of build shown by the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum BuildKind {
    /// A public release build with verified release provenance.
    Official,
    /// A build without verified public release provenance.
    Developer,
    /// A build kind added by a newer producer.
    #[serde(other)]
    Unknown,
}

/// Build-time values embedded into the executable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductBuildInfo {
    /// Product version string.
    pub product_version: Option<String>,
    /// Whether public release provenance was verified by the build pipeline.
    pub build_kind: BuildKind,
    /// Source commit recorded by the build pipeline.
    pub commit: Option<String>,
    /// Build time in UTC using RFC 3339, if supplied by the build pipeline.
    pub built_at_utc: Option<String>,
    /// Target operating-system name recorded for this executable.
    pub target_os: Option<String>,
    /// Target CPU architecture recorded for this executable.
    pub target_cpu: Option<String>,
    /// Application database format version, if available.
    pub app_db_format: Option<u32>,
    /// Snapshot file format version, if available.
    pub snapshot_format: Option<u32>,
    /// Support-matrix revision embedded into this executable.
    pub support_matrix_revision: Option<String>,
}

/// Operating-system family observed by the current process.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SupportOsFamily {
    /// Microsoft Windows.
    Windows,
    /// Apple macOS.
    Macos,
    /// A Linux distribution.
    Linux,
    /// The operating system could not be identified or is not recognized.
    #[serde(other)]
    Unknown,
}

/// CPU architecture observed for the current process.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SupportCpu {
    /// 64-bit x86.
    X64,
    /// 64-bit ARM.
    Arm64,
    /// A known architecture outside the supported labels above.
    Other,
    /// The process architecture could not be identified.
    #[serde(other)]
    Unknown,
}

/// Exact platform identity fields used for support-matrix matching.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SupportEnvironmentIdentity {
    /// Operating-system family. Unknown is never used as a wildcard.
    pub os_family: SupportOsFamily,
    /// Operating-system release or distribution release identifier.
    pub release: Option<String>,
    /// Edition or distribution identifier.
    pub edition: Option<String>,
    /// CPU architecture of the running process.
    pub cpu: SupportCpu,
}

impl SupportEnvironmentIdentity {
    /// Returns true only when every key used by the support matrix is known.
    pub fn is_complete(&self) -> bool {
        self.os_family != SupportOsFamily::Unknown
            && self
                .release
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
            && self
                .edition
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
            && !matches!(self.cpu, SupportCpu::Unknown | SupportCpu::Other)
    }
}

/// Support status recorded from a published matrix or current capability probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SupportStatus {
    /// The exact combination passed the release's required native checks.
    OfficiallyVerified,
    /// The combination is supported with recorded limits or conditions.
    Conditional,
    /// A native check found the combination unsupported.
    UnsupportedFound,
    /// No verification result exists for the exact combination.
    NotVerified,
    /// The required value or evidence could not be established.
    #[serde(other)]
    Unknown,
}

/// Current value for one functional capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SupportCapabilityState {
    /// The capability is available in the current environment.
    Supported,
    /// The capability is available with a known limitation.
    Limited,
    /// The capability is known to be unavailable.
    Unsupported,
    /// The capability does not apply to this platform or operation.
    NotApplicable,
    /// The capability could not be established.
    #[serde(other)]
    Unknown,
}

/// Security-support evidence recorded for one support-matrix combination.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SecuritySupportBasis {
    /// Source reference supporting the vendor's security-support status.
    pub source: Option<String>,
    /// Date the source was checked, formatted as YYYY-MM-DD.
    pub checked_on: Option<String>,
    /// Whether the vendor reports general or extended support.
    pub kind: Option<SecuritySupportKind>,
}

/// Vendor security-support category.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SecuritySupportKind {
    /// Standard vendor support period.
    General,
    /// Vendor extended-support period.
    Extended,
    /// A value added by a newer producer.
    #[serde(other)]
    Unknown,
}

/// Support-matrix identity for one operating-system and CPU combination.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SupportCombinationIdentity {
    /// Operating-system family for this row.
    pub os_family: SupportOsFamily,
    /// Release or distribution release identifier.
    pub release: Option<String>,
    /// Edition or distribution identifier.
    pub edition: Option<String>,
    /// CPU architecture for this row.
    pub cpu: SupportCpu,
}

impl SupportCombinationIdentity {
    /// Checks exact equality against a complete runtime identity.
    pub fn matches(&self, observed: &SupportEnvironmentIdentity) -> bool {
        observed.is_complete()
            && self.os_family == observed.os_family
            && self.release == observed.release
            && self.edition == observed.edition
            && self.cpu == observed.cpu
    }
}

/// A feature-specific support entry recorded by the release process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SupportFeatureRow {
    /// Stable feature-area key.
    pub area: String,
    /// Support-table status for this feature area.
    pub status: SupportStatus,
    /// Recorded conditions or restrictions, preserved as source text.
    #[serde(default)]
    pub conditions: Vec<String>,
}

/// One exact supported or observed operating-system and CPU combination.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SupportCombination {
    /// Exact identity of this combination.
    pub identity: SupportCombinationIdentity,
    /// Status derived from native test records for this exact combination.
    pub status: SupportStatus,
    /// Vendor security-support source and check date, when recorded.
    pub security_support: Option<SecuritySupportBasis>,
    /// Detailed configuration and execution-form identifiers covered by tests.
    #[serde(default)]
    pub verified_details: Vec<String>,
    /// References to the native test records used to derive this row's status.
    #[serde(default)]
    pub verification_records: Vec<String>,
    /// Feature-area support rows for this exact combination.
    #[serde(default)]
    pub features: Vec<SupportFeatureRow>,
}

/// Read-only matrix embedded by the release build.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportMatrix {
    /// Matrix revision.
    pub revision: Option<String>,
    /// Date the matrix was prepared, formatted as YYYY-MM-DD.
    pub as_of: Option<String>,
    /// Combination rows backed by exact-platform records.
    #[serde(default)]
    pub combinations: Vec<SupportCombination>,
}

/// Capability observed through a platform adapter at request time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportCapabilityObservation {
    /// Stable feature-area key.
    pub area: String,
    /// Current operating-system capability value.
    pub state: SupportCapabilityState,
    /// Stable reason key explaining a limited or unknown value.
    pub reason_key: Option<String>,
}

/// Narrow operating-system port for support information commands.
///
/// Implementations return only observed values. They must use Unknown for an
/// OS release, edition, architecture, or capability they cannot identify.
pub trait SupportEnvironmentPort: Send + Sync {
    /// Identifies the current process environment for exact matrix matching.
    fn identity(&self) -> SupportEnvironmentIdentity;

    /// Returns capability values that can be established without prompting.
    fn capabilities(&self) -> Vec<SupportCapabilityObservation>;
}

/// Support details for one feature row in the current environment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SupportFeatureAssessment {
    /// Stable feature-area key.
    pub area: String,
    /// Status recorded in the matching support-table row.
    pub support_table_status: SupportStatus,
    /// Current capability value reported by the platform adapter.
    pub current_capability: SupportCapabilityState,
    /// Recorded support conditions for the feature area.
    #[serde(default)]
    pub conditions: Vec<String>,
    /// Stable reason key for an unknown or limited current capability.
    pub reason_key: Option<String>,
}

/// Support information prepared for a local CLI or GUI consumer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SupportReport {
    /// Embedded support-matrix revision, if present.
    pub matrix_revision: Option<String>,
    /// Date the embedded matrix was prepared.
    pub as_of: Option<String>,
    /// Current process environment identity.
    pub environment: SupportEnvironmentIdentity,
    /// Status of the exact current OS and CPU combination.
    pub combination_status: SupportStatus,
    /// Security-support source and date, separate from combination status.
    pub security_support: Option<SecuritySupportBasis>,
    /// Feature-area support and current capability rows.
    pub features: Vec<SupportFeatureAssessment>,
    /// All rows from the embedded matrix, in their published order.
    pub combinations: Vec<SupportCombination>,
    /// Stable reason key for an unknown or unverified combination.
    pub reason_key: Option<String>,
}

/// Product notices embedded in the executable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductNotices {
    /// License text embedded from the repository license file.
    pub license: Option<String>,
    /// Third-party notices text generated or reviewed for this build.
    pub third_party: Option<String>,
    /// Commit from which the notices were generated, if known.
    pub source_commit: Option<String>,
}

/// All product metadata resources passed to a local entry point at assembly.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedProductInfo {
    /// Build metadata for this executable.
    pub build: ProductBuildInfo,
    /// Embedded notices available in this executable.
    pub notices: ProductNotices,
    /// Support matrix embedded into this executable, if available.
    pub support_matrix: Option<SupportMatrix>,
}
