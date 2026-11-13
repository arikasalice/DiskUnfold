//! F15 startup status and access command bodies.
use serde::{Deserialize, Serialize};

/// Reads the status computed for the current application startup.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct StartupStatusRequest {}

impl<'de> Deserialize<'de> for StartupStatusRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.as_object().is_some_and(serde_json::Map::is_empty) {
            Ok(Self {})
        } else {
            Err(serde::de::Error::custom("expected an empty object"))
        }
    }
}

/// A startup check that contributes one row to the F15 report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub enum StartupCheck {
    /// Integrity of bundled resources required by the application.
    #[serde(rename = "resource_integrity")]
    ResourceIntegrity,
    /// Availability of the native webview runtime.
    #[serde(rename = "webview")]
    Webview,
    /// Data-directory readiness, database opening, and format compatibility.
    #[serde(rename = "data_format_and_open")]
    DataFormatAndOpen,
    /// Cleanup of temporary files left by an interrupted operation.
    #[serde(rename = "temporary_files")]
    TemporaryFiles,
    /// Comparison of the observed environment with the embedded support matrix.
    #[serde(rename = "support_matrix")]
    SupportMatrix,
    /// Restrictions derived from observed platform capabilities.
    #[serde(rename = "capability")]
    Capability,
    /// The current session's full-disk-access status.
    #[serde(rename = "full_disk_access")]
    FullDiskAccess,
    /// A check added by a newer producer that this reader does not recognize.
    #[serde(rename = "unknown", other)]
    Unknown,
}

/// Outcome reported for one startup check.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub enum StartupCheckOutcome {
    /// The observed condition meets its check.
    #[serde(rename = "pass")]
    Pass,
    /// The check found a supported limitation that does not block startup.
    #[serde(rename = "limited")]
    Limited,
    /// The environment is outside the combinations verified by the support matrix.
    #[serde(rename = "outside_verified")]
    OutsideVerified,
    /// The available evidence cannot establish the check result.
    #[serde(rename = "unverifiable")]
    Unverifiable,
    /// A technical prerequisite failed before the application window opened.
    #[serde(rename = "blocked")]
    Blocked,
    /// This check does not apply to the current environment.
    #[serde(rename = "not_applicable")]
    NotApplicable,
    /// A newer producer returned an outcome that this reader does not recognize.
    #[serde(rename = "unknown", other)]
    Unknown,
}

/// Aggregate grade for startup checks that do not block before a window opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub enum StartupGrade {
    /// All applicable advisory checks passed.
    #[serde(rename = "pass")]
    Pass,
    /// At least one check observed a supported limitation.
    #[serde(rename = "limited")]
    Limited,
    /// The current environment is outside verified support combinations.
    #[serde(rename = "outside_verified")]
    OutsideVerified,
    /// At least one applicable check could not be verified.
    #[serde(rename = "unverifiable")]
    Unverifiable,
    /// A newer producer returned a grade that this reader does not recognize.
    #[serde(rename = "unknown", other)]
    Unknown,
}

/// A check result in the fixed startup report order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StartupCheckResult {
    /// The check represented by this row.
    pub check: StartupCheck,
    /// The observed outcome. Unknown values remain safe to display generically.
    pub result: StartupCheckOutcome,
    /// An open reason key when the check needs to explain its outcome.
    pub reason_key: Option<String>,
}

/// Access status associated with the current GUI startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub enum StartupAccessState {
    /// The required access was observed as available.
    #[serde(rename = "confirmed")]
    Confirmed,
    /// The required access was observed as denied.
    #[serde(rename = "denied")]
    Denied,
    /// Access control does not apply to this platform or operation.
    #[serde(rename = "not_applicable")]
    NotApplicable,
    /// The access state could not be observed, or the producer returned a new value.
    #[serde(rename = "unknown", other)]
    Unknown,
}

/// Complete read-only F15 status for one application startup.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StartupStatus {
    /// The most severe applicable advisory result.
    pub grade: StartupGrade,
    /// Results in the published startup check order.
    pub checks: Vec<StartupCheckResult>,
    /// Whether the application created the user's startup database.
    pub first_run: bool,
    /// Full-disk-access state observed for the current startup.
    pub access: StartupAccessState,
    /// Whether analysis entry is allowed in the current session.
    pub analysis_allowed: bool,
}

/// Selects one of the two startup access actions defined by IF-20.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StartupAccessAction {
    /// Ask the operating system to open its privacy settings for DiskUnfold.
    OpenPrivacySettings,
    /// Read the current full-disk-access state again.
    Recheck,
}

/// Requests a startup access action.
#[derive(Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupAccessRequest {
    /// Action to perform.
    pub action: StartupAccessAction,
}

/// Result of an IF-20 startup access action.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StartupAccessResponse {
    /// Observed full-disk-access state; unknown is never treated as denied.
    pub access: StartupAccessState,
    /// Whether analysis entry is allowed under the observed access state.
    pub analysis_allowed: bool,
    /// Whether the operating system privacy settings screen was opened.
    pub settings_opened: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_request_accepts_only_an_empty_object() {
        assert!(serde_json::from_value::<StartupStatusRequest>(json!({})).is_ok());
        assert!(serde_json::from_value::<StartupStatusRequest>(json!({"future": true})).is_err());
        assert!(serde_json::from_value::<StartupStatusRequest>(json!([])).is_err());
    }

    #[test]
    fn access_request_uses_the_published_actions_and_rejects_unknown_fields() {
        assert!(
            serde_json::from_value::<StartupAccessRequest>(json!({"action":"recheck"}))
                .is_ok_and(|request| request.action == StartupAccessAction::Recheck)
        );
        assert!(
            serde_json::from_value::<StartupAccessRequest>(json!({
                "action":"open_privacy_settings"
            }))
            .is_ok_and(|request| request.action == StartupAccessAction::OpenPrivacySettings)
        );
        assert!(
            serde_json::from_value::<StartupAccessRequest>(json!({
                "action":"recheck", "extra":1
            }))
            .is_err()
        );
    }

    #[test]
    fn unknown_response_values_deserialize_to_safe_unknown_variants() {
        assert!(
            serde_json::from_value::<StartupGrade>(json!("future_grade"))
                .is_ok_and(|value| value == StartupGrade::Unknown)
        );
        assert!(
            serde_json::from_value::<StartupAccessState>(json!("future_access"))
                .is_ok_and(|value| value == StartupAccessState::Unknown)
        );
        assert!(
            serde_json::to_value(StartupAccessState::Unknown)
                .is_ok_and(|value| value == json!("unknown"))
        );
        assert!(
            serde_json::from_value::<StartupCheckOutcome>(json!("future_result"))
                .is_ok_and(|value| value == StartupCheckOutcome::Unknown)
        );
        assert!(
            serde_json::to_value(StartupCheckOutcome::Unknown)
                .is_ok_and(|value| value == json!("unknown"))
        );
    }
}
