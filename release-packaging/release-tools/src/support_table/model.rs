//! Input documents for the support-table generator.

use serde::Deserialize;

/// Reason key: a combination row has no complete security-support basis.
pub const SUPPORT_BASIS_MISSING: &str = "release.support_basis_missing";
/// Reason key: a security-support basis has not been confirmed for release.
pub const SUPPORT_BASIS_UNCONFIRMED: &str = "release.support_basis_unconfirmed";
/// Reason key: a test record or table row is malformed or has no matching row.
pub const SUPPORT_EVIDENCE_INVALID: &str = "release.support_evidence_invalid";

/// The checks a record must pass, all of them, for an officially verified row.
pub const REQUIRED_CHECKS: [&str; 3] = ["native_build", "native_tests", "installed_start"];

/// The support-table section of targets.json.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportTableSpec {
    /// Date the table was prepared, YYYY-MM-DD.
    pub as_of: String,
    /// Groups expanded into combination rows.
    pub groups: Vec<GroupSpec>,
}

/// An OS family with releases, editions and CPUs to cross.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSpec {
    /// windows, macos or linux.
    pub os: String,
    /// Releases, each with its own security-support basis.
    pub releases: Vec<ReleaseSpec>,
    /// Edition identifiers (Windows EditionID, macos, or Linux ID).
    pub editions: Vec<String>,
    /// CPUs: x64 or arm64.
    pub cpus: Vec<String>,
}

/// One release of an OS family.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseSpec {
    /// Encoded release key.
    pub release: String,
    /// Vendor security-support basis; absent when it could not be confirmed.
    pub security_basis: Option<BasisSpec>,
}

/// Security-support basis recorded for a release.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BasisSpec {
    /// general or extended.
    pub kind: Option<String>,
    /// Vendor lifecycle document.
    pub source: Option<String>,
    /// Date the source was checked, YYYY-MM-DD.
    pub checked_on: Option<String>,
    /// True only after a person confirmed the basis against the source.
    #[serde(default)]
    pub confirmed: bool,
    /// Vendor end date when the vendor publishes one. Shown, never used at run time.
    pub vendor_end_date: Option<String>,
}

/// Wrapper matching the whole targets.json file.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetsFile {
    /// Support table specification.
    pub support_table: SupportTableSpec,
}

/// One real test record for one exact combination.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRecord {
    /// Unique record id referenced by the table row.
    pub id: String,
    /// windows, macos or linux.
    pub os: String,
    /// Encoded release key.
    pub release: String,
    /// Edition identifier.
    pub edition: String,
    /// x64 or arm64.
    pub cpu: String,
    /// Date of the run, YYYY-MM-DD.
    pub observed_on: String,
    /// Concrete OS configuration that was exercised.
    pub os_detail: String,
    /// Device or runner used.
    pub device: String,
    /// Execution form, for example portable exe or deb in a container.
    pub execution_form: String,
    /// Check name to passed, failed, limited or not_run.
    pub checks: std::collections::BTreeMap<String, String>,
    /// Known limits of this run.
    #[serde(default)]
    pub limits: Vec<String>,
    /// Where the evidence can be found.
    pub evidence: String,
    /// Feature rows observed in this run.
    #[serde(default)]
    pub features: Vec<RecordFeature>,
}

/// A feature observation inside a test record.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordFeature {
    /// Feature area key.
    pub area: String,
    /// officially_verified, conditional, unsupported_found or not_verified.
    pub status: String,
    /// Conditions or restrictions.
    #[serde(default)]
    pub conditions: Vec<String>,
}
