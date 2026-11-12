//! Shared settings persistence boundary.
use std::collections::BTreeMap;

use serde_json::Value;

/// Current registered logical settings descriptor schema.
pub const SETTINGS_SCHEMA_VERSION: u32 = 3;

/// Validated F03 capacity, frozen when a scan is admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedTableLimit(usize);

impl SharedTableLimit {
    pub const MIN_ENTRIES: usize = 100_000;
    pub const MAX_ENTRIES: usize = 10_000_000;
    pub const DEFAULT_ENTRIES: usize = 1_000_000;

    /// Accepts the inclusive bounds of the published setting.
    pub fn new(entries: usize) -> Option<Self> {
        (Self::MIN_ENTRIES..=Self::MAX_ENTRIES)
            .contains(&entries)
            .then_some(Self(entries))
    }

    /// Returns the validated number of entries in each table.
    pub fn entries(self) -> usize {
        self.0
    }
}

impl Default for SharedTableLimit {
    fn default() -> Self {
        Self(Self::DEFAULT_ENTRIES)
    }
}

/// Published settings-screen group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingGroup {
    /// G1: general and display settings.
    GeneralDisplay,
    /// G2: accessibility settings.
    Accessibility,
    /// G3: analysis settings.
    Analysis,
    /// G4: cleanup and safety settings.
    CleanupSafety,
    /// G5: history, retention and privacy settings.
    HistoryPrivacy,
    /// G6: export and sharing settings.
    ExportSharing,
    /// G7: external integration settings.
    ExternalIntegration,
    /// G8: advanced limits and resources.
    AdvancedResources,
}

/// Published settings-screen visibility level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingLevel {
    /// Visible when the group is opened.
    Basic,
    /// Visible in detailed settings and search.
    Detailed,
    /// Visible in advanced settings and search.
    Advanced,
}

/// A published group and level, kept together as one classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingPlacement {
    /// Settings-screen group.
    pub group: SettingGroup,
    /// Settings-screen visibility level.
    pub level: SettingLevel,
}

/// Message resource keys for a setting's name and description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingPresentation {
    /// Localized name resource key.
    pub name_key: String,
    /// Localized description resource key.
    pub description_key: String,
    /// Published placement, or `None` when no classification is established.
    pub placement: Option<SettingPlacement>,
}

impl SettingPresentation {
    /// Derives the registered resource keys from the stable setting key.
    pub fn for_setting(key: &str) -> Self {
        Self {
            name_key: format!("settings.{key}.name"),
            description_key: format!("settings.{key}.description"),
            placement: None,
        }
    }

    /// Binds a published screen group and level to this presentation.
    pub fn with_placement(mut self, group: SettingGroup, level: SettingLevel) -> Self {
        self.placement = Some(SettingPlacement { group, level });
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingDescriptor {
    /// Localized presentation keys; values remain in language resources.
    pub presentation: SettingPresentation,
    pub key: String,
    pub owner: String,
    pub default: Value,
    pub kind: SettingValueKind,
    pub editable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingValueKind {
    Boolean,
    Integer { min: i64, max: i64 },
    Choice(Vec<Value>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsSnapshot {
    pub revision: u64,
    pub schema_version: u32,
    pub values: BTreeMap<String, Value>,
}

/// `None` removes a user override; defaults are never persisted.
pub type SettingsChanges = BTreeMap<String, Option<Value>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSource {
    Screen,
    Import,
    Reset,
    Owner,
    Migration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsCommit {
    pub source: SettingsSource,
    pub timestamp_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsStoreError {
    Conflict,
    Busy,
    Storage,
}

pub trait SettingsStore {
    fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError>;
    fn commit(
        &mut self,
        expected: u64,
        changes: &SettingsChanges,
        context: SettingsCommit,
    ) -> Result<u64, SettingsStoreError>;
}
