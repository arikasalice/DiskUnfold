use super::*;
use crate::space::{self, SIZE_UNIT, SIZE_UNIT_SYSTEM};
use du_core_contracts::settings::SettingsSource;
use serde_json::json;

struct Store(SettingsSnapshot);
impl SettingsStore for Store {
    fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
        Ok(self.0.clone())
    }
    fn commit(
        &mut self,
        revision: u64,
        changes: &SettingsChanges,
        _: SettingsCommit,
    ) -> Result<u64, SettingsStoreError> {
        if self.0.revision != revision {
            return Err(SettingsStoreError::Conflict);
        }
        for (key, value) in changes {
            if let Some(value) = value {
                self.0.values.insert(key.clone(), value.clone());
            } else {
                self.0.values.remove(key);
            }
        }
        self.0.revision += 1;
        Ok(self.0.revision)
    }
}
fn context() -> SettingsCommit {
    SettingsCommit {
        source: SettingsSource::Screen,
        timestamp_ms: 1,
    }
}
fn setup(system: Option<&str>, unit: Option<&str>) -> Result<SettingsService<Store>, String> {
    let mut rows = space::display_descriptors(true);
    rows.extend(crate::scope::descriptors());
    let registry = Registry::new(rows).map_err(|e| format!("{e:?}"))?;
    let mut snapshot = SettingsSnapshot {
        schema_version: SETTINGS_SCHEMA_VERSION,
        ..SettingsSnapshot::default()
    };
    if let Some(value) = system {
        snapshot
            .values
            .insert(SIZE_UNIT_SYSTEM.into(), json!(value));
    }
    if let Some(value) = unit {
        snapshot.values.insert(SIZE_UNIT.into(), json!(value));
    }
    SettingsService::open(registry, Store(snapshot)).map_err(|e| format!("{e:?}"))
}

#[test]
fn pair_validation_uses_effective_values_and_commits_combined_changes_atomically()
-> Result<(), String> {
    let mut service = setup(Some("iec"), Some("MiB"))?;
    let system_only = SettingsChanges::from([(SIZE_UNIT_SYSTEM.into(), Some(json!("si")))]);
    assert_eq!(
        service.review(0, &system_only),
        Err(SettingsError::InvalidValue)
    );
    assert_eq!(service.snapshot().revision, 0);
    assert_eq!(service.store.0.values[SIZE_UNIT], json!("MiB"));
    let both = SettingsChanges::from([
        (SIZE_UNIT_SYSTEM.into(), Some(json!("si"))),
        (SIZE_UNIT.into(), Some(json!("MB"))),
    ]);
    let fingerprint = service.review(0, &both).map_err(|e| format!("{e:?}"))?;
    service
        .apply(0, &both, fingerprint, context())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(service.snapshot().revision, 1);
    assert_eq!(
        (
            service.display_units().ok_or("missing")?.system,
            service.display_units().ok_or("missing")?.unit
        ),
        ("si", "MB")
    );
    assert_eq!(service.store.0.values[SIZE_UNIT_SYSTEM], json!("si"));
    Ok(())
}

#[test]
fn defaults_auto_and_bytes_work_for_both_systems() -> Result<(), String> {
    use du_core_contracts::size_units::{IEC_UNITS, SI_UNITS};
    for (system, table) in [("si", &SI_UNITS), ("iec", &IEC_UNITS)] {
        for unit in table.symbols() {
            assert!(space::compatible_units(system, unit));
        }
    }
    assert!(!space::compatible_units("iec", "MB"));
    assert!(!space::compatible_units("si", "MiB"));
    for unit in ["auto", "B"] {
        let mut service = setup(None, Some(unit))?;
        let change = SettingsChanges::from([(SIZE_UNIT_SYSTEM.into(), Some(json!("si")))]);
        let hash = service.review(0, &change).map_err(|e| format!("{e:?}"))?;
        service
            .apply(0, &change, hash, context())
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(service.display_units().ok_or("missing")?.unit, unit);
    }
    let service = setup(None, None)?;
    assert_eq!(
        service.display_units(),
        Some(space::DisplayUnits {
            system: "iec",
            unit: "auto",
            warning: None
        })
    );
    Ok(())
}

#[test]
fn legacy_mismatch_projects_warning_preserves_values_and_allows_unrelated_edits()
-> Result<(), String> {
    let mut service = setup(Some("si"), Some("MiB"))?;
    assert_eq!(
        service.display_units(),
        Some(space::DisplayUnits {
            system: "si",
            unit: "auto",
            warning: Some("settings.value_reset")
        })
    );
    assert_eq!(
        service.restore_status().items[SIZE_UNIT].reason,
        Some("settings.value_reset")
    );
    assert_eq!(service.snapshot().values[SIZE_UNIT], json!("MiB"));
    let unrelated = SettingsChanges::from([("scope.remember_recent".into(), Some(json!(true)))]);
    let hash = service
        .review(0, &unrelated)
        .map_err(|e| format!("{e:?}"))?;
    service
        .apply(0, &unrelated, hash, context())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(service.store.0.values[SIZE_UNIT], json!("MiB"));
    assert_eq!(service.display_units().ok_or("missing")?.unit, "auto");
    let fix = SettingsChanges::from([(SIZE_UNIT.into(), Some(json!("MB")))]);
    let hash = service.review(1, &fix).map_err(|e| format!("{e:?}"))?;
    service
        .apply(1, &fix, hash, context())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(service.display_units().ok_or("missing")?.warning, None);
    Ok(())
}

#[test]
fn apply_rechecks_pair_after_review_without_partial_commit() -> Result<(), String> {
    let mut service = setup(Some("iec"), Some("auto"))?;
    let change = SettingsChanges::from([(SIZE_UNIT.into(), Some(json!("MiB")))]);
    let hash = service.review(0, &change).map_err(|e| format!("{e:?}"))?;
    service
        .snapshot
        .values
        .insert(SIZE_UNIT_SYSTEM.into(), json!("si"));
    assert_eq!(
        service.apply(0, &change, hash, context()),
        Err(SettingsError::InvalidValue)
    );
    assert_eq!(service.store.0.revision, 0);
    assert_eq!(service.store.0.values[SIZE_UNIT], json!("auto"));
    Ok(())
}

#[test]
fn restore_does_not_quarantine_individually_valid_mismatched_values() -> Result<(), String> {
    let registry = Registry::new(space::display_descriptors(true)).map_err(|e| format!("{e:?}"))?;
    let called = std::cell::Cell::new(false);
    let snapshot = SettingsSnapshot {
        schema_version: SETTINGS_SCHEMA_VERSION,
        ..SettingsSnapshot::default()
    };
    let service = SettingsService::restore(
        registry,
        Store(snapshot),
        |_| {
            Ok((
                4,
                SETTINGS_SCHEMA_VERSION,
                [
                    (SIZE_UNIT_SYSTEM.into(), "\"si\"".into()),
                    (SIZE_UNIT.into(), "\"MiB\"".into()),
                ]
                .into(),
            ))
        },
        |_, _, _| {
            called.set(true);
            Err(SettingsStoreError::Storage)
        },
    );
    assert!(!called.get());
    assert!(service.restore_status().invalid_keys.is_empty());
    assert_eq!(
        service.restore_status().items[SIZE_UNIT].reason,
        Some("settings.value_reset")
    );
    assert_eq!(service.snapshot().values[SIZE_UNIT], json!("MiB"));
    assert_eq!(service.display_units().ok_or("missing")?.unit, "auto");
    assert_eq!(service.snapshot().revision, 4);
    Ok(())
}
