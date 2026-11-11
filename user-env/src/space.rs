//! F03 settings descriptors for composition into the shared settings registry.

use crate::{Descriptor, ValueKind};
use du_core_contracts::settings::{SettingsSnapshot, SharedTableLimit};
use du_core_contracts::size_units::{IEC_UNITS, SI_UNITS};
use serde_json::json;

/// The registered key for the per-table identity capacity.
pub const SHARED_TABLE_MAX_ENTRIES: &str = "space.shared_table_max_entries";
/// The registered key for the size unit system.
pub const SIZE_UNIT_SYSTEM: &str = "display.size_unit_system";
/// The registered key for automatic or fixed size unit selection.
pub const SIZE_UNIT: &str = "display.size_unit";
/// The registered key for the default size basis.
pub const DEFAULT_BASIS: &str = "space.default_basis";

/// Returns the F03 descriptor. Applying it to future jobs is the caller's responsibility.
pub fn descriptors() -> Vec<Descriptor> {
    let mut result = vec![Descriptor {
        presentation: du_core_contracts::settings::SettingPresentation::for_setting(
            SHARED_TABLE_MAX_ENTRIES,
        ),
        key: SHARED_TABLE_MAX_ENTRIES.into(),
        owner: "F03".into(),
        default: json!(SharedTableLimit::DEFAULT_ENTRIES),
        kind: ValueKind::Integer {
            min: SharedTableLimit::MIN_ENTRIES as i64,
            max: SharedTableLimit::MAX_ENTRIES as i64,
        },
        editable: true,
    }];
    result.extend(display_descriptors(cfg!(target_os = "windows")));
    result
}

/// Returns display descriptors using the specified platform convention.
/// Windows uses IEC; macOS and the supported Ubuntu target use SI.
pub fn display_descriptors(windows: bool) -> Vec<Descriptor> {
    let mut symbols = vec!["auto"];
    for symbol in SI_UNITS.symbols().iter().chain(IEC_UNITS.symbols()) {
        if !symbols.contains(symbol) {
            symbols.push(symbol);
        }
    }
    let choice = |key: &str, default: &str, values: &[&str]| Descriptor {
        presentation: du_core_contracts::settings::SettingPresentation::for_setting(key),
        key: key.into(),
        owner: "F03".into(),
        default: json!(default),
        kind: ValueKind::Choice(values.iter().map(|value| json!(value)).collect()),
        editable: true,
    };
    vec![
        choice(
            SIZE_UNIT_SYSTEM,
            if windows { "iec" } else { "si" },
            &["si", "iec"],
        ),
        choice(SIZE_UNIT, "auto", &symbols),
        choice(
            DEFAULT_BASIS,
            "allocated",
            &["logical", "allocated", "unique_physical"],
        ),
    ]
}

/// Resolves the F03 override in a stored snapshot. An absent override uses the
/// descriptor default; an unreadable or out-of-range override requires recovery.
pub fn restore_shared_table_limit(snapshot: &SettingsSnapshot) -> Option<SharedTableLimit> {
    if snapshot.schema_version != du_core_contracts::settings::SETTINGS_SCHEMA_VERSION {
        return None;
    }
    match snapshot.values.get(SHARED_TABLE_MAX_ENTRIES) {
        None => Some(SharedTableLimit::default()),
        Some(value) => SharedTableLimit::new(usize::try_from(value.as_u64()?).ok()?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{SettingsError, SettingsService};
    use crate::{Registry, accepts};
    use du_core_contracts::settings::{
        SettingsChanges, SettingsCommit, SettingsSnapshot, SettingsStore, SettingsStoreError,
    };

    struct Store;
    impl SettingsStore for Store {
        fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
            Ok(SettingsSnapshot {
                schema_version: 2,
                ..SettingsSnapshot::default()
            })
        }
        fn commit(
            &mut self,
            _: u64,
            _: &SettingsChanges,
            _: SettingsCommit,
        ) -> Result<u64, SettingsStoreError> {
            Err(SettingsStoreError::Storage)
        }
    }

    #[test]
    fn display_choices_follow_platform_defaults_and_reject_invalid_values() -> Result<(), String> {
        for (windows, expected) in [(true, "iec"), (false, "si")] {
            let rows = display_descriptors(windows);
            let find = |key: &str| rows.iter().find(|row| row.key == key).ok_or("missing");
            let system = find("display.size_unit_system")?;
            assert_eq!(system.default, json!(expected));
            assert!(accepts(&system.kind, &json!("si")));
            assert!(accepts(&system.kind, &json!("iec")));
            assert!(!accepts(&system.kind, &json!("SI")));
            let unit = find("display.size_unit")?;
            assert_eq!(unit.default, json!("auto"));
            for symbol in SI_UNITS.symbols().iter().chain(IEC_UNITS.symbols()) {
                assert!(accepts(&unit.kind, &json!(symbol)));
            }
            for value in ["auto", "B", "kB", "PB", "KiB", "PiB"] {
                assert!(accepts(&unit.kind, &json!(value)));
            }
            assert!(!accepts(&unit.kind, &json!("KB")));
            let basis = find("space.default_basis")?;
            assert_eq!(basis.default, json!("allocated"));
            for value in ["logical", "allocated", "unique_physical"] {
                assert!(accepts(&basis.kind, &json!(value)));
            }
            assert!(!accepts(&basis.kind, &json!("physical")));
        }
        Ok(())
    }

    #[test]
    fn display_settings_reject_invalid_values_and_stale_reviews() -> Result<(), String> {
        let registry = Registry::new(display_descriptors(true)).map_err(|e| format!("{e:?}"))?;
        let service = SettingsService::open(registry, Store).map_err(|e| format!("{e:?}"))?;
        let valid = SettingsChanges::from([("display.size_unit".into(), Some(json!("PiB")))]);
        assert!(service.review(0, &valid).is_ok());
        assert_eq!(service.review(1, &valid), Err(SettingsError::StaleVersion));
        let invalid =
            SettingsChanges::from([("space.default_basis".into(), Some(json!("physical")))]);
        assert_eq!(
            service.review(0, &invalid),
            Err(SettingsError::InvalidValue)
        );
        Ok(())
    }

    #[test]
    fn display_restore_quarantines_invalid_value_and_keeps_valid_override() -> Result<(), String> {
        let registry = Registry::new(display_descriptors(true)).map_err(|e| format!("{e:?}"))?;
        let service = SettingsService::restore(
            registry,
            Store,
            |_| {
                Ok((
                    3,
                    2,
                    [
                        ("display.size_unit".into(), "\"bogus\"".into()),
                        ("display.size_unit_system".into(), "\"si\"".into()),
                    ]
                    .into(),
                ))
            },
            |_, revision, invalid| {
                assert_eq!(revision, 3);
                assert!(invalid.contains_key("display.size_unit"));
                Ok(4)
            },
        );
        assert_eq!(
            service.snapshot().values.get("display.size_unit_system"),
            Some(&json!("si"))
        );
        assert!(!service.snapshot().values.contains_key("display.size_unit"));
        assert_eq!(service.snapshot().revision, 4);
        assert_eq!(
            service.restore_status().invalid_keys,
            vec!["display.size_unit"]
        );
        Ok(())
    }

    #[test]
    fn descriptor_has_published_default_and_inclusive_range() -> Result<(), String> {
        let descriptors = descriptors();
        let descriptor = descriptors.first().ok_or("missing descriptor")?;
        assert_eq!(descriptor.default, json!(1_000_000));
        for value in [100_000, 1_000_000, 10_000_000] {
            assert!(accepts(&descriptor.kind, &json!(value)));
        }
        for value in [99_999, 10_000_001] {
            assert!(!accepts(&descriptor.kind, &json!(value)));
        }
        assert!(!accepts(&descriptor.kind, &json!(1.5)));
        let registry = Registry::new(descriptors).map_err(|error| format!("{error:?}"))?;
        let catalog_snapshot = SettingsSnapshot::default();
        assert_eq!(
            registry
                .catalog(&catalog_snapshot)
                .into_iter()
                .find(|row| row.descriptor.key == SHARED_TABLE_MAX_ENTRIES)
                .ok_or("missing")?
                .value,
            &json!(1_000_000)
        );
        let service =
            SettingsService::open(registry, Store).map_err(|error| format!("{error:?}"))?;
        for value in [100_000, 10_000_000] {
            let changes =
                SettingsChanges::from([(SHARED_TABLE_MAX_ENTRIES.into(), Some(json!(value)))]);
            assert!(service.review(0, &changes).is_ok());
        }
        for value in [99_999, 10_000_001] {
            let changes =
                SettingsChanges::from([(SHARED_TABLE_MAX_ENTRIES.into(), Some(json!(value)))]);
            assert_eq!(
                service.review(0, &changes),
                Err(SettingsError::InvalidValue)
            );
        }
        Ok(())
    }

    #[test]
    fn restores_valid_overrides_without_consulting_unrelated_keys() {
        let mut snapshot = SettingsSnapshot {
            schema_version: 2,
            ..SettingsSnapshot::default()
        };
        snapshot
            .values
            .insert("unregistered.future".into(), json!(true));
        assert_eq!(
            restore_shared_table_limit(&snapshot),
            Some(SharedTableLimit::default())
        );
        for value in [100_000, 10_000_000] {
            snapshot
                .values
                .insert(SHARED_TABLE_MAX_ENTRIES.into(), json!(value));
            assert_eq!(
                restore_shared_table_limit(&snapshot),
                SharedTableLimit::new(value)
            );
        }
        snapshot
            .values
            .insert(SHARED_TABLE_MAX_ENTRIES.into(), json!(99_999));
        assert_eq!(restore_shared_table_limit(&snapshot), None);
        snapshot.values.clear();
        snapshot.schema_version = 3;
        assert_eq!(restore_shared_table_limit(&snapshot), None);
    }
}
