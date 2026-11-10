//! Settings service validation, snapshot and commit regressions.
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
fn catalog_reflects_commits_and_not_failed_or_stale_changes() -> Result<(), String> {
    let mut service = setup()?;
    let before = service.snapshot().clone();
    let (revision, rows) = service.catalog();
    assert_eq!(revision, 0);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].descriptor.owner, "F01");
    assert_eq!(rows[0].value, &json!(true));
    assert!(rows[0].uses_default);
    assert_eq!(service.snapshot(), &before);
    let changes = SettingsChanges::from([("scope.remember_recent".into(), Some(json!(false)))]);
    let digest = service.review(0, &changes).map_err(|e| format!("{e:?}"))?;
    service.store.fail = Some(SettingsStoreError::Storage);
    assert!(service.apply(0, &changes, digest, context()).is_err());
    let (revision, rows) = service.catalog();
    assert_eq!(
        (revision, rows[0].value, rows[0].uses_default),
        (0, &json!(true), true)
    );
    service.store.fail = None;
    service
        .apply(0, &changes, digest, context())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        service.apply(0, &changes, digest, context()),
        Err(SettingsError::StaleVersion)
    );
    let (revision, rows) = service.catalog();
    assert_eq!(
        (revision, rows[0].value, rows[0].uses_default),
        (1, &json!(false), false)
    );
    let reset = SettingsChanges::from([("scope.remember_recent".into(), None)]);
    let digest = service.review(1, &reset).map_err(|e| format!("{e:?}"))?;
    service
        .apply(1, &reset, digest, context())
        .map_err(|e| format!("{e:?}"))?;
    let (revision, rows) = service.catalog();
    assert_eq!(
        (revision, rows[0].value, rows[0].uses_default),
        (2, &json!(true), true)
    );
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
