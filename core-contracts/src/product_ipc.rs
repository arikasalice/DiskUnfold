//! F15 product and support command bodies for HLD rev 116 IF-01 to IF-04.
//!
//! The HLD publishes the command names and the information each command
//! carries, but not the wire field names. The names below are therefore
//! implementation choices that await publication in the DLD.
use crate::{
    Id,
    product_support::{BuildKind, SupportReport},
};
use serde::{Deserialize, Serialize};

/// How the running executable was installed or delivered.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum InstallForm {
    /// Installed from a Windows Installer package.
    #[serde(rename = "msi")]
    Msi,
    /// A portable executable that was not installed.
    #[serde(rename = "portable")]
    Portable,
    /// Installed as a packaged MSIX application.
    #[serde(rename = "msix")]
    Msix,
    /// Copied from a macOS disk image into an application bundle.
    #[serde(rename = "dmg")]
    Dmg,
    /// Installed from a Debian package.
    #[serde(rename = "deb")]
    Deb,
    /// Extracted from a tar.gz archive.
    #[serde(rename = "tar_gz")]
    TarGz,
    /// The install form could not be established, or a newer producer added a value.
    #[serde(rename = "unknown", other)]
    Unknown,
}

/// Embedded notice document kinds addressable through IF-02.
///
/// There is deliberately no catch-all variant: an unknown kind fails body
/// validation and is reported as INVALID_ARGUMENT.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum NoticeKind {
    /// The product license text.
    #[serde(rename = "LICENSE")]
    License,
    /// The third-party notices generated for this build.
    #[serde(rename = "THIRD-PARTY-NOTICES")]
    ThirdPartyNotices,
}

macro_rules! empty_request {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
        pub struct $name {}

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = serde_json::Value::deserialize(deserializer)?;
                if value.as_object().is_some_and(serde_json::Map::is_empty) {
                    Ok(Self {})
                } else {
                    Err(serde::de::Error::custom("expected an empty object"))
                }
            }
        }
    };
}

empty_request!(
    /// IF-01 `app.product_info` request. The command takes no input.
    ProductInfoRequest
);
empty_request!(
    /// IF-04 `app.support_info` request. The command takes no input.
    SupportInfoRequest
);

/// Availability of one embedded notice document.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct NoticeAvailability {
    /// Document kind.
    pub kind: NoticeKind,
    /// Whether the executable embeds a body for this document.
    pub available: bool,
}

/// IF-01 response: embedded build values and observed install form.
///
/// A `None` value means the build or the platform did not supply it. Consumers
/// must show it as unavailable and never substitute a default.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProductInfo {
    /// Product version string.
    pub product_version: Option<String>,
    /// Official or developer build marker; a developer build is never verified output.
    pub build_kind: BuildKind,
    /// Source commit recorded at build time.
    pub commit: Option<String>,
    /// Build time in UTC (RFC 3339).
    pub built_at_utc: Option<String>,
    /// Target operating system recorded at build time.
    pub target_os: Option<String>,
    /// Target CPU architecture recorded at build time.
    pub target_cpu: Option<String>,
    /// Observed install form; unknown is never promoted to a guess.
    pub install_form: InstallForm,
    /// `product.install_form_unknown` when the install form was not established.
    pub install_form_reason_key: Option<String>,
    /// Application database format version.
    pub app_db_format: Option<u32>,
    /// Snapshot file format version.
    pub snapshot_format: Option<u32>,
    /// Embedded support-matrix revision.
    pub support_matrix_revision: Option<String>,
    /// Embedded support-matrix preparation date (YYYY-MM-DD).
    pub support_matrix_as_of: Option<String>,
    /// Notice documents with their availability, in a fixed order.
    pub notices: Vec<NoticeAvailability>,
}

/// IF-02 request: selects one embedded notice document.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoticesRequest {
    /// Document kind to read.
    pub kind: NoticeKind,
}

/// IF-02 response: the embedded document text and the commit it belongs to.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct NoticesResponse {
    /// Requested document kind.
    pub kind: NoticeKind,
    /// Document body, or `None` when this build embeds no such document.
    pub text: Option<String>,
    /// Commit from which the embedded notices were generated, when recorded.
    pub source_commit: Option<String>,
}

/// IF-03 request: asks for the executable digest of the running process.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutableDigestRequest {
    /// Correlation identifier the caller uses to match the response.
    pub correlation_id: Id,
}

/// IF-03 response. The digest is a computed fact and is never a verification result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ExecutableDigestResponse {
    /// Echo of the request correlation identifier.
    pub correlation_id: Id,
    /// Lowercase hexadecimal SHA-256 of the running executable, if it could be read.
    pub sha256: Option<String>,
    /// `product.digest_unavailable` when the digest could not be computed.
    pub reason_key: Option<String>,
}

/// IF-04 response body: the evaluated support report.
pub type SupportInfo = SupportReport;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_requests_reject_any_other_body() {
        assert!(serde_json::from_value::<ProductInfoRequest>(json!({})).is_ok());
        assert!(serde_json::from_value::<SupportInfoRequest>(json!({})).is_ok());
        assert!(serde_json::from_value::<ProductInfoRequest>(json!({"x": 1})).is_err());
        assert!(serde_json::from_value::<SupportInfoRequest>(json!(null)).is_err());
    }

    #[test]
    fn notice_kind_uses_the_published_document_names_and_rejects_others()
    -> Result<(), serde_json::Error> {
        let request: NoticesRequest =
            serde_json::from_value(json!({"kind":"THIRD-PARTY-NOTICES"}))?;
        assert_eq!(request.kind, NoticeKind::ThirdPartyNotices);
        assert!(serde_json::from_value::<NoticesRequest>(json!({"kind":"license"})).is_err());
        assert!(serde_json::from_value::<NoticesRequest>(json!({"kind":"NOTICE"})).is_err());
        Ok(())
    }

    #[test]
    fn unknown_install_form_values_read_as_unknown() -> Result<(), serde_json::Error> {
        let form: InstallForm = serde_json::from_value(json!("flatpak"))?;
        assert_eq!(form, InstallForm::Unknown);
        assert_eq!(serde_json::to_value(InstallForm::TarGz)?, json!("tar_gz"));
        Ok(())
    }
}
