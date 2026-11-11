//! Immutable startup restoration report for registered settings.

use std::collections::BTreeMap;

use du_core_contracts::settings::SettingsSnapshot;

use crate::Registry;

/// Source used for a registered setting at startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreSource {
    /// A validated override was loaded from storage.
    Stored,
    /// The descriptor default is in effect.
    Default,
}

/// Startup outcome for a single registered setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreItem {
    /// The source selected for this startup.
    pub source: RestoreSource,
    /// Failure reason, or none when the value was restored or simply absent.
    pub reason: Option<&'static str>,
}

/// Startup outcome, separate from the application database open result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreStatus {
    /// Registered keys whose raw values failed validation, including failed quarantine.
    pub invalid_keys: Vec<String>,
    /// Category-wide reason when the stored snapshot could not be restored.
    pub failure_reason: Option<&'static str>,
    /// Whether settings writes are disabled for this startup.
    pub write_locked: bool,
    /// Stored settings revision, if its category header was readable.
    pub stored_revision: Option<u64>,
    /// Stored settings schema, if its category header was readable.
    pub stored_schema: Option<u32>,
    /// Schema version supported by the application.
    pub app_schema: u32,
    /// True only after a successful schema migration.
    pub migrated: bool,
    /// Outcomes for registered descriptors; unknown keys are not interpreted.
    pub items: BTreeMap<String, RestoreItem>,
}

impl Default for RestoreStatus {
    fn default() -> Self {
        Self {
            invalid_keys: Vec::new(),
            failure_reason: None,
            write_locked: false,
            stored_revision: None,
            stored_schema: None,
            app_schema: du_core_contracts::settings::SETTINGS_SCHEMA_VERSION,
            migrated: false,
            items: BTreeMap::new(),
        }
    }
}

impl RestoreStatus {
    pub(super) fn record_items(&mut self, registry: &Registry, snapshot: &SettingsSnapshot) {
        for key in registry.descriptors.keys() {
            let invalid = self.invalid_keys.iter().any(|candidate| candidate == key);
            let stored = !invalid && snapshot.values.contains_key(key);
            let source = if stored {
                RestoreSource::Stored
            } else {
                RestoreSource::Default
            };
            let reason = if stored {
                None
            } else if invalid {
                Some("settings.value_invalid")
            } else {
                self.failure_reason
            };
            self.items
                .insert(key.clone(), RestoreItem { source, reason });
        }
    }
}
