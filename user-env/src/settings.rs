//! Review and atomic commit for supported setting kinds.
use du_core_contracts::settings::{
    SETTINGS_SCHEMA_VERSION, SettingsChanges, SettingsCommit, SettingsSnapshot, SettingsStore,
    SettingsStoreError,
};

use crate::{CatalogEntry, Registry};
mod restore_status;
pub mod search;
pub use restore_status::{RestoreItem, RestoreSource, RestoreStatus};

/// Registered additive-only transition for the F03 display descriptors.
pub const EMPTY_SCHEMA_TRANSITION: (u32, u32) = (1, SETTINGS_SCHEMA_VERSION);

/// Builds the shared startup catalog from each feature's registered descriptors.
pub fn builtin_descriptors(
    configured_languages: &[&str],
) -> Result<Vec<crate::Descriptor>, crate::locale::DecisionError> {
    Ok(crate::locale::descriptors(configured_languages)?
        .into_iter()
        .chain(crate::scope::descriptors())
        .chain(crate::space::descriptors())
        .collect())
}

#[derive(Debug, PartialEq, Eq)]
pub enum SettingsError {
    UnknownKey,
    NotEditable,
    InvalidValue,
    StaleVersion,
    StaleReview,
    Store(SettingsStoreError),
    Encoding,
    SchemaNewer,
    SchemaUnsupported,
    InvalidStoredValue,
    RestoreLocked,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Changed {
    pub revision: u64,
    pub keys: Vec<String>,
}

pub struct SettingsService<S> {
    registry: Registry,
    store: S,
    snapshot: SettingsSnapshot,
    restore_status: RestoreStatus,
}

impl<S: SettingsStore> SettingsService<S> {
    pub fn open(registry: Registry, mut store: S) -> Result<Self, SettingsError> {
        let snapshot = store.load().map_err(SettingsError::Store)?;
        if snapshot.schema_version > SETTINGS_SCHEMA_VERSION {
            return Err(SettingsError::SchemaNewer);
        }
        if snapshot.schema_version != SETTINGS_SCHEMA_VERSION {
            return Err(SettingsError::SchemaUnsupported);
        }
        for (key, value) in &snapshot.values {
            let descriptor = registry
                .descriptors
                .get(key)
                .ok_or(SettingsError::InvalidStoredValue)?;
            if !crate::accepts(&descriptor.kind, value) {
                return Err(SettingsError::InvalidStoredValue);
            }
        }
        let mut status = RestoreStatus {
            stored_revision: Some(snapshot.revision),
            stored_schema: Some(snapshot.schema_version),
            ..RestoreStatus::default()
        };
        status.record_items(&registry, &snapshot);
        mark_display_warning(&mut status, &registry, &snapshot);
        Ok(Self {
            registry,
            store,
            snapshot,
            restore_status: status,
        })
    }

    /// Restores known keys and atomically quarantines invalid known values.
    ///
    /// The caller opens the application database first; an open failure is not
    /// converted into a settings-category failure here. `quarantine` must
    /// compare the revision and values, move all supplied rows atomically, and
    /// return the new revision. Unknown keys are preserved without validation.
    pub fn restore<L, F>(registry: Registry, mut store: S, load: L, quarantine: F) -> Self
    where
        L: FnOnce(
            &mut S,
        ) -> Result<
            (u64, u32, std::collections::BTreeMap<String, String>),
            SettingsStoreError,
        >,
        F: FnOnce(
            &mut S,
            u64,
            &std::collections::BTreeMap<String, String>,
        ) -> Result<u64, SettingsStoreError>,
    {
        let mut status = RestoreStatus::default();
        let mut invalid = std::collections::BTreeMap::new();
        let loaded = load(&mut store);
        if let Ok((revision, schema, _)) = &loaded {
            status.stored_revision = Some(*revision);
            status.stored_schema = Some(*schema);
        }
        let mut snapshot = match loaded {
            Ok((revision, SETTINGS_SCHEMA_VERSION, rows)) => {
                let mut values = std::collections::BTreeMap::new();
                for (key, raw) in rows {
                    let value = serde_json::from_str::<serde_json::Value>(&raw);
                    if let Some(descriptor) = registry.descriptors.get(&key)
                        && value
                            .as_ref()
                            .map_or(true, |v| !crate::accepts(&descriptor.kind, v))
                    {
                        invalid.insert(key, raw);
                    } else if let Ok(value) = value {
                        values.insert(key, value);
                    }
                }
                SettingsSnapshot {
                    revision,
                    schema_version: SETTINGS_SCHEMA_VERSION,
                    values,
                }
            }
            Ok((_, schema_version, _)) => {
                status.write_locked = true;
                status.failure_reason = Some(if schema_version > SETTINGS_SCHEMA_VERSION {
                    "settings.schema_newer"
                } else {
                    "settings.schema_unsupported"
                });
                SettingsSnapshot {
                    schema_version: SETTINGS_SCHEMA_VERSION,
                    ..SettingsSnapshot::default()
                }
            }
            Err(_) => {
                status.write_locked = true;
                status.failure_reason = Some("settings.restore_failed");
                SettingsSnapshot {
                    schema_version: SETTINGS_SCHEMA_VERSION,
                    ..SettingsSnapshot::default()
                }
            }
        };
        if !status.write_locked && !invalid.is_empty() {
            status.invalid_keys = invalid.keys().cloned().collect();
            match quarantine(&mut store, snapshot.revision, &invalid) {
                Ok(revision) => {
                    snapshot.revision = revision;
                }
                Err(_) => {
                    status.write_locked = true;
                    status.failure_reason = Some("settings.restore_failed");
                }
            }
        }
        status.record_items(&registry, &snapshot);
        mark_display_warning(&mut status, &registry, &snapshot);
        Self {
            registry,
            store,
            snapshot,
            restore_status: status,
        }
    }

    /// Records a successfully committed GUI-only logical schema transition.
    pub fn record_migration(&mut self, from: u32) {
        self.restore_status.migrated = true;
        self.restore_status.stored_schema = Some(from);
    }

    /// Returns the immutable startup restoration outcome.
    pub fn restore_status(&self) -> &RestoreStatus {
        &self.restore_status
    }

    pub fn snapshot(&self) -> &SettingsSnapshot {
        &self.snapshot
    }

    /// Returns the effective display unit settings without rewriting stored values.
    /// An incompatible stored pair projects to automatic units with a warning.
    /// Returns `None` if the descriptors or individually valid values are unavailable.
    pub fn display_units(&self) -> Option<crate::space::DisplayUnits<'_>> {
        display_units(&self.registry, &self.snapshot, None)
    }

    /// Returns the F06 minimum drawing area when its descriptor was explicitly registered.
    pub fn visualization_settings(&self) -> crate::visualization_settings::VisualizationSettings {
        let mut settings = crate::visualization_settings::project(&self.snapshot);
        if let Some(reason) = self.restore_status.failure_reason {
            settings.warning_key = Some(reason);
        } else if self
            .restore_status
            .invalid_keys
            .iter()
            .any(|key| key == crate::visualization_settings::TREEMAP_MIN_AREA_PX2)
        {
            settings.warning_key = Some("settings.value_reset");
        }
        settings
    }

    /// Returns the catalog and its revision from the same immutable settings snapshot.
    ///
    /// Each value is a validated override or the registered default. Reading the
    /// catalog does not change the stored values or advance the revision.
    pub fn catalog(&self) -> (u64, Vec<CatalogEntry<'_>>) {
        (
            self.snapshot.revision,
            self.registry.catalog(&self.snapshot),
        )
    }

    pub fn review(
        &self,
        revision: u64,
        changes: &SettingsChanges,
    ) -> Result<[u8; 32], SettingsError> {
        if self.restore_status.write_locked {
            return Err(SettingsError::RestoreLocked);
        }
        if revision != self.snapshot.revision {
            return Err(SettingsError::StaleVersion);
        }
        self.validate(changes)?;
        fingerprint(revision, &self.normalized(changes))
    }

    pub fn apply(
        &mut self,
        revision: u64,
        changes: &SettingsChanges,
        reviewed: [u8; 32],
        context: SettingsCommit,
    ) -> Result<Changed, SettingsError> {
        if self.restore_status.write_locked {
            return Err(SettingsError::RestoreLocked);
        }
        if revision != self.snapshot.revision {
            return Err(SettingsError::StaleVersion);
        }
        let changes = self.normalized(changes);
        if fingerprint(revision, &changes)? != reviewed {
            return Err(SettingsError::StaleReview);
        }
        self.validate(&changes)?;
        let next = self
            .store
            .commit(revision, &changes, context)
            .map_err(|e| match e {
                SettingsStoreError::Conflict => SettingsError::StaleVersion,
                other => SettingsError::Store(other),
            })?;
        for (key, value) in &changes {
            match value {
                Some(value) => {
                    self.snapshot.values.insert(key.clone(), value.clone());
                }
                None => {
                    self.snapshot.values.remove(key);
                }
            }
        }
        self.snapshot.revision = next;
        Ok(Changed {
            revision: next,
            keys: changes.keys().cloned().collect(),
        })
    }

    fn normalized(&self, changes: &SettingsChanges) -> SettingsChanges {
        changes
            .iter()
            .map(|(key, value)| {
                let default = self.registry.descriptors.get(key).map(|item| &item.default);
                let value = if value.as_ref() == default {
                    None
                } else {
                    value.clone()
                };
                (key.clone(), value)
            })
            .collect()
    }

    fn validate(&self, changes: &SettingsChanges) -> Result<(), SettingsError> {
        for (key, value) in changes {
            let descriptor = self
                .registry
                .descriptors
                .get(key)
                .ok_or(SettingsError::UnknownKey)?;
            if !descriptor.editable {
                return Err(SettingsError::NotEditable);
            }
            if value
                .as_ref()
                .is_some_and(|value| !crate::accepts(&descriptor.kind, value))
            {
                return Err(SettingsError::InvalidValue);
            }
        }
        if (changes.contains_key(crate::space::SIZE_UNIT_SYSTEM)
            || changes.contains_key(crate::space::SIZE_UNIT))
            && display_units(&self.registry, &self.snapshot, Some(changes))
                .is_none_or(|units| units.warning.is_some())
        {
            return Err(SettingsError::InvalidValue);
        }
        Ok(())
    }
}

fn display_units<'a>(
    registry: &'a Registry,
    snapshot: &'a SettingsSnapshot,
    changes: Option<&'a SettingsChanges>,
) -> Option<crate::space::DisplayUnits<'a>> {
    let resolve = |key: &str| {
        let default = &registry.descriptors.get(key)?.default;
        let value = match changes.and_then(|changes| changes.get(key)) {
            Some(Some(value)) => value,
            Some(None) => default,
            None => snapshot.values.get(key).unwrap_or(default),
        };
        value.as_str()
    };
    crate::space::project_display_units(
        resolve(crate::space::SIZE_UNIT_SYSTEM)?,
        resolve(crate::space::SIZE_UNIT)?,
    )
}

fn mark_display_warning(
    status: &mut RestoreStatus,
    registry: &Registry,
    snapshot: &SettingsSnapshot,
) {
    if display_units(registry, snapshot, None).is_some_and(|units| units.warning.is_some())
        && let Some(item) = status.items.get_mut(crate::space::SIZE_UNIT)
    {
        item.source = RestoreSource::Default;
        item.reason = Some("settings.value_reset");
    }
}

fn fingerprint(revision: u64, changes: &SettingsChanges) -> Result<[u8; 32], SettingsError> {
    let bytes = serde_json::to_vec(&(revision, changes)).map_err(|_| SettingsError::Encoding)?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

#[cfg(test)]
mod restore_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod display_units_tests;
