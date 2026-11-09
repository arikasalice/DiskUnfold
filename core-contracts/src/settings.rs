//! Shared settings persistence boundary.
use std::collections::BTreeMap;

use serde_json::Value;

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
