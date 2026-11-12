//! Tests for the opt-in F06 settings supplier.

use std::collections::BTreeMap;

use super::*;
use crate::{Registry, accepts};
use du_core_contracts::settings::{
    SettingsChanges, SettingsCommit, SettingsSource, SettingsStore, SettingsStoreError,
};

struct Store {
    snapshot: SettingsSnapshot,
    fail_next_commit: bool,
}

impl SettingsStore for Store {
    fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
        Ok(self.snapshot.clone())
    }

    fn commit(
        &mut self,
        expected: u64,
        changes: &SettingsChanges,
        _: SettingsCommit,
    ) -> Result<u64, SettingsStoreError> {
        if self.fail_next_commit {
            self.fail_next_commit = false;
            return Err(SettingsStoreError::Storage);
        }
        if self.snapshot.revision != expected {
            return Err(SettingsStoreError::Conflict);
        }
        for (key, value) in changes {
            match value {
                Some(value) => {
                    self.snapshot.values.insert(key.clone(), value.clone());
                }
                None => {
                    self.snapshot.values.remove(key);
                }
            }
        }
        self.snapshot.revision += 1;
        Ok(self.snapshot.revision)
    }
}

fn restored_with(
    registry: Registry,
    values: BTreeMap<String, Value>,
) -> crate::settings::SettingsService<Store> {
    let snapshot = SettingsSnapshot {
        schema_version: SETTINGS_SCHEMA_VERSION,
        values,
        ..SettingsSnapshot::default()
    };
    crate::settings::SettingsService::restore(
        registry,
        Store {
            snapshot,
            fail_next_commit: false,
        },
        |store| {
            Ok((
                store.snapshot.revision,
                store.snapshot.schema_version,
                store
                    .snapshot
                    .values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_string()))
                    .collect(),
            ))
        },
        |store, revision, invalid| {
            for key in invalid.keys() {
                store.snapshot.values.remove(key);
            }
            store.snapshot.revision = revision + 1;
            Ok(store.snapshot.revision)
        },
    )
}

fn effective(
    service: &crate::settings::SettingsService<Store>,
) -> Result<VisualizationSettings, String> {
    service
        .visualization_settings()
        .ok_or_else(|| "F06 descriptor is unavailable".to_owned())
}

#[test]
fn descriptor_has_published_default_bounds_without_invented_placement() -> Result<(), String> {
    let descriptor = descriptors()
        .into_iter()
        .next()
        .ok_or("missing descriptor")?;
    assert_eq!(descriptor.key, TREEMAP_MIN_AREA_PX2);
    assert_eq!(descriptor.default, json!(16));
    assert_eq!(descriptor.owner, "F06");
    assert_eq!(descriptor.presentation.placement, None);
    assert!(accepts(&descriptor.kind, &json!(4)));
    assert!(accepts(&descriptor.kind, &json!(256)));
    assert!(!accepts(&descriptor.kind, &json!(3)));
    assert!(!accepts(&descriptor.kind, &json!(257)));
    assert!(Registry::new([descriptor]).is_ok());
    let builtin = crate::settings::builtin_descriptors(&["en", "ko"])
        .map_err(|error| format!("{error:?}"))?;
    assert!(!builtin.iter().any(|item| item.key == TREEMAP_MIN_AREA_PX2));
    Ok(())
}

#[test]
fn projection_uses_default_and_snapshot_override() {
    let empty = SettingsSnapshot {
        schema_version: SETTINGS_SCHEMA_VERSION,
        ..SettingsSnapshot::default()
    };
    assert_eq!(
        project(&empty),
        VisualizationSettings {
            min_area_px2: 16,
            used_default: true,
            warning_key: None,
        }
    );

    let snapshot = SettingsSnapshot {
        revision: 2,
        schema_version: du_core_contracts::settings::SETTINGS_SCHEMA_VERSION,
        values: BTreeMap::from([(TREEMAP_MIN_AREA_PX2.into(), json!(64))]),
    };
    assert_eq!(
        project(&snapshot),
        VisualizationSettings {
            min_area_px2: 64,
            used_default: false,
            warning_key: None,
        }
    );
}

#[test]
fn projection_fails_closed_to_default_for_invalid_snapshot_values() {
    for value in [json!(3), json!(257), json!(-1), json!(16.5), Value::Null] {
        let snapshot = SettingsSnapshot {
            revision: 1,
            schema_version: du_core_contracts::settings::SETTINGS_SCHEMA_VERSION,
            values: BTreeMap::from([(TREEMAP_MIN_AREA_PX2.into(), value)]),
        };
        assert_eq!(
            project(&snapshot),
            VisualizationSettings {
                min_area_px2: 16,
                used_default: true,
                warning_key: Some("settings.value_reset"),
            }
        );
    }
}

#[test]
fn projection_reports_unsupported_snapshot_schema() {
    let snapshot = SettingsSnapshot::default();
    assert_eq!(
        project(&snapshot).warning_key,
        Some("settings.schema_unsupported")
    );
}

#[test]
fn service_rejects_unregistered_or_conflicting_descriptors() -> Result<(), String> {
    let builtin = Registry::new(
        crate::settings::builtin_descriptors(&["en", "ko"])
            .map_err(|error| format!("{error:?}"))?,
    )
    .map_err(|error| format!("{error:?}"))?;
    let preserved = restored_with(
        builtin,
        BTreeMap::from([(TREEMAP_MIN_AREA_PX2.into(), json!(64))]),
    );
    assert_eq!(
        preserved.snapshot().values.get(TREEMAP_MIN_AREA_PX2),
        Some(&json!(64))
    );
    assert_eq!(preserved.visualization_settings(), None);

    let mut wrong_owner = descriptors();
    wrong_owner[0].owner = "F99".into();
    let service = restored_with(
        Registry::new(wrong_owner).map_err(|error| format!("{error:?}"))?,
        BTreeMap::from([(TREEMAP_MIN_AREA_PX2.into(), json!(64))]),
    );
    assert_eq!(service.visualization_settings(), None);

    let mut wrong_kind = descriptors();
    wrong_kind[0].kind = ValueKind::Choice(vec![json!(16), json!(64)]);
    let service = restored_with(
        Registry::new(wrong_kind).map_err(|error| format!("{error:?}"))?,
        BTreeMap::from([(TREEMAP_MIN_AREA_PX2.into(), json!(64))]),
    );
    assert_eq!(service.visualization_settings(), None);
    Ok(())
}

#[test]
fn setting_review_commit_and_reset_freeze_values_per_snapshot() -> Result<(), String> {
    let registry = Registry::new(descriptors()).map_err(|error| format!("{error:?}"))?;
    let mut service = crate::settings::SettingsService::open(
        registry,
        Store {
            snapshot: SettingsSnapshot {
                schema_version: du_core_contracts::settings::SETTINGS_SCHEMA_VERSION,
                ..SettingsSnapshot::default()
            },
            fail_next_commit: false,
        },
    )
    .map_err(|error| format!("{error:?}"))?;
    let initial = service.snapshot().clone();

    for (revision, area) in [(0, 4), (1, 16), (2, 256)] {
        let changes = SettingsChanges::from([(TREEMAP_MIN_AREA_PX2.into(), Some(json!(area)))]);
        let reviewed = service
            .review(revision, &changes)
            .map_err(|error| format!("{error:?}"))?;
        service
            .apply(
                revision,
                &changes,
                reviewed,
                SettingsCommit {
                    source: SettingsSource::Screen,
                    timestamp_ms: i64::try_from(revision).map_err(|error| error.to_string())?,
                },
            )
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(effective(&service)?.min_area_px2, area);
        assert_eq!(effective(&service)?.used_default, area == 16);
    }

    for (area, revision) in [(3, 3), (257, 3)] {
        let changes = SettingsChanges::from([(TREEMAP_MIN_AREA_PX2.into(), Some(json!(area)))]);
        assert_eq!(
            service.review(revision, &changes),
            Err(crate::settings::SettingsError::InvalidValue)
        );
    }

    let reset = SettingsChanges::from([(TREEMAP_MIN_AREA_PX2.into(), None)]);
    let reviewed = service
        .review(3, &reset)
        .map_err(|error| format!("{error:?}"))?;
    service
        .apply(
            3,
            &reset,
            reviewed,
            SettingsCommit {
                source: SettingsSource::Reset,
                timestamp_ms: 4,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(effective(&service)?.min_area_px2, 16);
    assert!(effective(&service)?.used_default);
    assert_eq!(effective(&service)?.warning_key, None);
    assert!(!service.snapshot().values.contains_key(TREEMAP_MIN_AREA_PX2));
    assert_eq!(project(&initial).min_area_px2, 16);
    Ok(())
}

#[test]
fn restore_status_distinguishes_quarantined_value_from_quarantine_failure() -> Result<(), String> {
    let invalid = SettingsSnapshot {
        schema_version: SETTINGS_SCHEMA_VERSION,
        values: BTreeMap::from([(TREEMAP_MIN_AREA_PX2.into(), json!(3))]),
        ..SettingsSnapshot::default()
    };
    let registry = Registry::new(descriptors()).map_err(|error| format!("{error:?}"))?;
    let mut service = crate::settings::SettingsService::restore(
        registry,
        Store {
            snapshot: invalid.clone(),
            fail_next_commit: true,
        },
        |store| {
            Ok((
                store.snapshot.revision,
                store.snapshot.schema_version,
                store
                    .snapshot
                    .values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_string()))
                    .collect(),
            ))
        },
        |store, revision, invalid| {
            for key in invalid.keys() {
                store.snapshot.values.remove(key);
            }
            store.snapshot.revision = revision + 1;
            Ok(store.snapshot.revision)
        },
    );
    assert_eq!(
        effective(&service)?.warning_key,
        Some("settings.value_reset")
    );
    assert!(!service.restore_status().write_locked);
    let change = SettingsChanges::from([(TREEMAP_MIN_AREA_PX2.into(), Some(json!(16)))]);
    assert_eq!(
        service.review(0, &change),
        Err(crate::settings::SettingsError::StaleVersion)
    );
    assert_eq!(
        effective(&service)?.warning_key,
        Some("settings.value_reset")
    );
    let reviewed = service
        .review(1, &change)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        service.apply(
            1,
            &change,
            reviewed,
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 1,
            },
        ),
        Err(crate::settings::SettingsError::Store(
            SettingsStoreError::Storage
        ))
    );
    assert_eq!(
        effective(&service)?.warning_key,
        Some("settings.value_reset")
    );
    service
        .apply(
            1,
            &change,
            reviewed,
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 2,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(effective(&service)?.min_area_px2, 16);
    assert_eq!(effective(&service)?.warning_key, None);
    assert!(
        service
            .restore_status()
            .invalid_keys
            .iter()
            .any(|key| key == TREEMAP_MIN_AREA_PX2)
    );

    let mut additional_descriptors = descriptors();
    additional_descriptors.extend(crate::space::descriptors());
    let registry = Registry::new(additional_descriptors).map_err(|error| format!("{error:?}"))?;
    let mut values = BTreeMap::from([(TREEMAP_MIN_AREA_PX2.into(), json!(3))]);
    values.insert(crate::space::DEFAULT_BASIS.into(), json!("allocated"));
    let mut unrelated = crate::settings::SettingsService::restore(
        registry,
        Store {
            snapshot: SettingsSnapshot {
                schema_version: SETTINGS_SCHEMA_VERSION,
                values,
                ..SettingsSnapshot::default()
            },
            fail_next_commit: false,
        },
        |store| {
            Ok((
                store.snapshot.revision,
                store.snapshot.schema_version,
                store
                    .snapshot
                    .values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_string()))
                    .collect(),
            ))
        },
        |store, revision, invalid| {
            for key in invalid.keys() {
                store.snapshot.values.remove(key);
            }
            store.snapshot.revision = revision + 1;
            Ok(store.snapshot.revision)
        },
    );
    assert_eq!(
        effective(&unrelated)?.warning_key,
        Some("settings.value_reset")
    );
    let change =
        SettingsChanges::from([(crate::space::DEFAULT_BASIS.into(), Some(json!("logical")))]);
    let reviewed = unrelated
        .review(1, &change)
        .map_err(|error| format!("{error:?}"))?;
    unrelated
        .apply(
            1,
            &change,
            reviewed,
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 3,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        effective(&unrelated)?.warning_key,
        Some("settings.value_reset")
    );

    let registry = Registry::new(descriptors()).map_err(|error| format!("{error:?}"))?;
    let service = crate::settings::SettingsService::restore(
        registry,
        Store {
            snapshot: invalid,
            fail_next_commit: false,
        },
        |store| {
            Ok((
                store.snapshot.revision,
                store.snapshot.schema_version,
                store
                    .snapshot
                    .values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_string()))
                    .collect(),
            ))
        },
        |_, _, _| Err(SettingsStoreError::Storage),
    );
    assert_eq!(
        effective(&service)?.warning_key,
        Some("settings.restore_failed")
    );
    assert!(service.restore_status().write_locked);
    Ok(())
}
