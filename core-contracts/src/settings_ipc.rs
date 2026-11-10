//! Read-only GUI contract for the settings restored at startup.
use serde::{Deserialize, Serialize};

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
