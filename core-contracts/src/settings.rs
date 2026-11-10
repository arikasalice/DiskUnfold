//! Shared settings persistence boundary.
use std::collections::BTreeMap;

use serde_json::Value;

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

#[derive(Clone, Debug, PartialEq)]
pub struct SettingDescriptor {
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
