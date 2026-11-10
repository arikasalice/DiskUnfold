//! F03 settings descriptors for composition into the shared settings registry.

use crate::{Descriptor, ValueKind};
use serde_json::json;

/// The registered key for the per-table identity capacity.
pub const SHARED_TABLE_MAX_ENTRIES: &str = "space.shared_table_max_entries";

/// Returns the F03 descriptor. Applying it to future jobs is the caller's responsibility.
pub fn descriptors() -> Vec<Descriptor> {
    vec![Descriptor {
        key: SHARED_TABLE_MAX_ENTRIES.into(),
        owner: "F03".into(),
        default: json!(1_000_000),
        kind: ValueKind::Integer {
            min: 100_000,
            max: 10_000_000,
        },
        editable: true,
    }]
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
                schema_version: 1,
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
            registry.catalog(&catalog_snapshot)[0].value,
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
}
