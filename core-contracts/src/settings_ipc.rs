//! Read-only GUI contract for the settings restored at startup.
use serde::{Deserialize, Serialize};

/// Read-only request for the effective F03 display preferences.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct DisplayPreferencesRequest {}

impl<'de> Deserialize<'de> for DisplayPreferencesRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.is_object() {
            Ok(Self {})
        } else {
            Err(serde::de::Error::custom("expected an object"))
        }
    }
}

/// Published unit systems for size display.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DisplayUnitSystem {
    /// Decimal units based on 1000.
    Si,
    /// Binary units based on 1024.
    Iec,
}

/// Effective size display values from one settings snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DisplayPreferences {
    /// Canonical decimal representation of the settings revision.
    pub settings_revision: String,
    /// Selected unit system.
    pub unit_system: DisplayUnitSystem,
    /// Automatic selection or a symbol supported by the selected system.
    pub unit: String,
    /// Default size basis for new views.
    pub default_basis: crate::SizeBasis,
    /// Settings restoration or unit-pair warning, if present.
    pub warning_key: Option<String>,
}

/// An empty request for the report associated with this application startup.
#[derive(Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct SettingsRestoreRequest {}

/// Whether a validated override or a default was used at startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SettingsRestoreSource {
    /// A validated override was loaded from storage.
    Restored,
    /// The registered default is in use.
    Default,
}

/// Outcome for a registered setting, with no raw setting value attached.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsRestoreItem {
    /// Stable key of the registered setting.
    pub key: String,
    /// Source selected when restoring this setting.
    pub source: SettingsRestoreSource,
    /// Reason for falling back to a default, if a failure occurred.
    pub reason_key: Option<String>,
}

/// An immutable settings restoration report for the running application.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsRestoreStatus {
    /// Restoration outcome for each registered setting.
    pub items: Vec<SettingsRestoreItem>,
    /// Absent if the stored schema could not be read.
    pub stored_schema: Option<u32>,
    /// Settings schema supported by this application.
    pub app_schema: u32,
    /// Whether startup successfully migrated the stored settings.
    pub migrated: bool,
    /// Whether the startup outcome disallows normal settings writes.
    pub write_locked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_preferences_request_accepts_only_objects_and_ignores_extensions() {
        for value in [json!({}), json!({"future": [1]})] {
            assert!(serde_json::from_value::<DisplayPreferencesRequest>(value).is_ok());
        }
        for value in [json!([]), json!(null), json!(0), json!("object")] {
            assert!(serde_json::from_value::<DisplayPreferencesRequest>(value).is_err());
        }
    }

    #[test]
    fn display_preferences_revision_and_required_null_warning_serialize()
    -> Result<(), serde_json::Error> {
        let response = DisplayPreferences {
            settings_revision: u64::MAX.to_string(),
            unit_system: DisplayUnitSystem::Iec,
            unit: "auto".into(),
            default_basis: crate::SizeBasis::UniquePhysical,
            warning_key: None,
        };
        assert_eq!(
            serde_json::to_value(response)?,
            json!({"settingsRevision":"18446744073709551615", "unitSystem":"iec", "unit":"auto", "defaultBasis":"unique_physical", "warningKey":null})
        );
        Ok(())
    }
}
