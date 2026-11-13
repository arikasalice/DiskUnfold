//! Shared settings commit notification body (F14 IF-07).

use serde::{Deserialize, Serialize};

/// When a saved setting becomes effective.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SettingsApplyTiming {
    /// Redraw from the committed setting now.
    Immediate,
    /// Supply the value to the next query.
    NextQuery,
    /// Supply the value to the next job.
    NextJob,
    /// Apply after the next app start.
    NextStart,
    /// Apply through the owning feature's review.
    OwnerReview,
}

/// Product entry point that committed the setting changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SettingsChangeSource {
    /// Settings changed by a desktop user.
    Screen,
    /// A settings import applied its accepted entries.
    Import,
    /// A user reset settings to defaults.
    Reset,
    /// The owning feature committed a reviewed setting.
    Owner,
    /// A permitted MCP command committed its settings change.
    Mcp,
}

/// One changed key and its application timing. Setting values stay in storage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsChangedKey {
    /// Registered settings key.
    pub key: String,
    /// Application timing assigned by the owner.
    pub apply_timing: SettingsApplyTiming,
}

/// Body of `settings.changed`. The event envelope carries timestamp and correlation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsChangedBody {
    /// Previous settings revision as a canonical unsigned decimal string.
    pub previous_settings_revision: String,
    /// Committed settings revision as a canonical unsigned decimal string.
    pub settings_revision: String,
    /// The distinct changed keys and their application timing.
    pub changes: Vec<SettingsChangedKey>,
    /// Source of this commit.
    pub source: SettingsChangeSource,
}
