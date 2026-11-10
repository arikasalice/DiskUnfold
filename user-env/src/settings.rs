//! Review and atomic commit for supported setting kinds.
use du_core_contracts::settings::{
    SettingsChanges, SettingsCommit, SettingsSnapshot, SettingsStore, SettingsStoreError,
};

use crate::Registry;
mod restore_status;
pub use restore_status::{RestoreItem, RestoreSource, RestoreStatus};

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
        if snapshot.schema_version > 1 {
            return Err(SettingsError::SchemaNewer);
        }
        if snapshot.schema_version != 1 {
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
            Ok((revision, 1, rows)) => {
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
                    schema_version: 1,
                    values,
                }
            }
            Ok((_, schema_version, _)) => {
                status.write_locked = true;
                status.failure_reason = Some(if schema_version > 1 {
                    "settings.schema_newer"
                } else {
                    "settings.schema_unsupported"
                });
                SettingsSnapshot {
                    schema_version: 1,
                    ..SettingsSnapshot::default()
                }
            }
            Err(_) => {
                status.write_locked = true;
                status.failure_reason = Some("settings.restore_failed");
                SettingsSnapshot {
                    schema_version: 1,
                    ..SettingsSnapshot::default()
                }
            }
        };
        if !status.write_locked && !invalid.is_empty() {
            match quarantine(&mut store, snapshot.revision, &invalid) {
                Ok(revision) => {
                    snapshot.revision = revision;
                    status.invalid_keys = invalid.keys().cloned().collect();
                }
                Err(_) => {
                    status.write_locked = true;
                    status.failure_reason = Some("settings.restore_failed");
                    snapshot = SettingsSnapshot {
                        schema_version: 1,
                        ..SettingsSnapshot::default()
                    };
                }
            }
        }
        status.record_items(&registry, &snapshot);
        Self {
            registry,
            store,
            snapshot,
            restore_status: status,
        }
    }

    /// Returns the immutable startup restoration outcome.
    pub fn restore_status(&self) -> &RestoreStatus {
        &self.restore_status
    }

    pub fn snapshot(&self) -> &SettingsSnapshot {
        &self.snapshot
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
        Ok(())
    }
}

fn fingerprint(revision: u64, changes: &SettingsChanges) -> Result<[u8; 32], SettingsError> {
    let bytes = serde_json::to_vec(&(revision, changes)).map_err(|_| SettingsError::Encoding)?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

#[cfg(test)]
mod restore_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Descriptor, ValueKind};
    use du_core_contracts::settings::{SettingsSource, SettingsStoreError};
    use serde_json::json;

    struct Fake {
        snapshot: SettingsSnapshot,
        fail: Option<SettingsStoreError>,
    }
    impl SettingsStore for Fake {
        fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
            Ok(self.snapshot.clone())
        }
        fn commit(
            &mut self,
            expected: u64,
            changes: &SettingsChanges,
            _: SettingsCommit,
        ) -> Result<u64, SettingsStoreError> {
            if let Some(error) = &self.fail {
                return Err(error.clone());
            }
            if self.snapshot.revision != expected {
                return Err(SettingsStoreError::Conflict);
            }
            for (key, value) in changes {
                if let Some(value) = value {
                    self.snapshot.values.insert(key.clone(), value.clone());
                } else {
                    self.snapshot.values.remove(key);
                }
            }
            self.snapshot.revision += 1;
            Ok(self.snapshot.revision)
        }
    }

    fn setup() -> Result<SettingsService<Fake>, String> {
        let descriptor = Descriptor {
            key: "scope.remember_recent".into(),
            owner: "F01".into(),
            default: json!(true),
            kind: ValueKind::Boolean,
            editable: true,
        };
        let registry = Registry::new([descriptor]).map_err(|e| format!("{e:?}"))?;
        let snapshot = SettingsSnapshot {
            schema_version: 1,
            ..SettingsSnapshot::default()
        };
        SettingsService::open(
            registry,
            Fake {
                snapshot,
                fail: None,
            },
        )
        .map_err(|e| format!("{e:?}"))
    }
    fn context() -> SettingsCommit {
        SettingsCommit {
            source: SettingsSource::Screen,
            timestamp_ms: 100,
        }
    }

    #[test]
    fn review_commit_validation_and_failures_leave_snapshot_unchanged() -> Result<(), String> {
        let mut service = setup()?;
        let valid = SettingsChanges::from([("scope.remember_recent".into(), Some(json!(false)))]);
        let bad = SettingsChanges::from([
            ("scope.remember_recent".into(), Some(json!(false))),
            ("unknown".into(), Some(json!(2))),
        ]);
        assert_eq!(service.review(0, &bad), Err(SettingsError::UnknownKey));
        let invalid = SettingsChanges::from([("scope.remember_recent".into(), Some(json!(123)))]);
        assert_eq!(
            service.review(0, &invalid),
            Err(SettingsError::InvalidValue)
        );
        let digest = service.review(0, &valid).map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            service.apply(0, &valid, [0; 32], context()),
            Err(SettingsError::StaleReview)
        );
        service.store.fail = Some(SettingsStoreError::Storage);
        assert_eq!(
            service.apply(0, &valid, digest, context()),
            Err(SettingsError::Store(SettingsStoreError::Storage))
        );
        assert_eq!(service.snapshot.revision, 0);
        assert!(service.snapshot.values.is_empty());
        service.store.fail = None;
        let event = service
            .apply(0, &valid, digest, context())
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            (event.revision, event.keys),
            (1, vec!["scope.remember_recent".into()])
        );
        assert_eq!(
            service.apply(0, &valid, digest, context()),
            Err(SettingsError::StaleVersion)
        );
        let reset = SettingsChanges::from([("scope.remember_recent".into(), Some(json!(true)))]);
        let digest = service.review(1, &reset).map_err(|e| format!("{e:?}"))?;
        service
            .apply(1, &reset, digest, context())
            .map_err(|e| format!("{e:?}"))?;
        assert!(service.snapshot.values.is_empty());
        assert!(service.store.snapshot.values.is_empty());
        Ok(())
    }

    #[test]
    fn concurrent_store_change_does_not_replace_snapshot() -> Result<(), String> {
        let mut service = setup()?;
        let changes = SettingsChanges::from([("scope.remember_recent".into(), Some(json!(false)))]);
        let digest = service.review(0, &changes).map_err(|e| format!("{e:?}"))?;
        service.store.snapshot.revision = 1;
        assert_eq!(
            service.apply(0, &changes, digest, context()),
            Err(SettingsError::StaleVersion)
        );
        assert_eq!(service.snapshot.revision, 0);
        assert!(service.snapshot.values.is_empty());
        Ok(())
    }

    #[test]
    fn unvalidated_stored_values_and_schema_mismatches_are_not_exposed() -> Result<(), String> {
        let descriptor = Descriptor {
            key: "scope.remember_recent".into(),
            owner: "F01".into(),
            default: json!(true),
            kind: ValueKind::Boolean,
            editable: true,
        };
        for version in [0, 2] {
            let registry = Registry::new([descriptor.clone()]).map_err(|e| format!("{e:?}"))?;
            let snapshot = SettingsSnapshot {
                schema_version: version,
                ..SettingsSnapshot::default()
            };
            let result = SettingsService::open(
                registry,
                Fake {
                    snapshot,
                    fail: None,
                },
            );
            if version == 0 {
                assert!(matches!(result, Err(SettingsError::SchemaUnsupported)));
            } else {
                assert!(matches!(result, Err(SettingsError::SchemaNewer)));
            }
        }
        for (key, value) in [
            ("scope.remember_recent", json!(123)),
            ("unknown", json!(true)),
        ] {
            let registry = Registry::new([descriptor.clone()]).map_err(|e| format!("{e:?}"))?;
            let snapshot = SettingsSnapshot {
                revision: 3,
                schema_version: 1,
                values: [(key.into(), value)].into(),
            };
            let result = SettingsService::open(
                registry,
                Fake {
                    snapshot,
                    fail: None,
                },
            );
            assert!(matches!(result, Err(SettingsError::InvalidStoredValue)));
        }
        Ok(())
    }
}
