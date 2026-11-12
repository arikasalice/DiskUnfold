//! Published F06 view settings supplied from the shared settings snapshot.

use du_core_contracts::settings::{SETTINGS_SCHEMA_VERSION, SettingsSnapshot};
use serde_json::{Value, json};

use crate::{Descriptor, ValueKind};

/// Registered key for the minimum area used when laying out treemap shapes.
pub const TREEMAP_MIN_AREA_PX2: &str = "display.treemap.min_area_px2";

/// Published inclusive bounds for the minimum treemap shape area.
pub const TREEMAP_MIN_AREA_PX2_MIN: u32 = 4;
pub const TREEMAP_MIN_AREA_PX2_MAX: u32 = 256;
pub const TREEMAP_MIN_AREA_PX2_DEFAULT: u32 = 16;

/// Effective view settings projected from one immutable settings snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualizationSettings {
    /// Minimum shape area, in square pixels.
    pub min_area_px2: u32,
    /// True when the snapshot has no usable override and the published default is used.
    pub used_default: bool,
    /// Restore warning owned by the settings service, if one applies.
    pub warning_key: Option<&'static str>,
}

/// Registers the published F06 settings whose editable bounds are specified.
pub fn descriptors() -> Vec<Descriptor> {
    vec![Descriptor {
        presentation: du_core_contracts::settings::SettingPresentation::for_setting(
            TREEMAP_MIN_AREA_PX2,
        ),
        key: TREEMAP_MIN_AREA_PX2.into(),
        owner: "F06".into(),
        default: json!(TREEMAP_MIN_AREA_PX2_DEFAULT),
        kind: ValueKind::Integer {
            min: i64::from(TREEMAP_MIN_AREA_PX2_MIN),
            max: i64::from(TREEMAP_MIN_AREA_PX2_MAX),
        },
        editable: true,
    }]
}

/// Projects a validated setting override, falling back to the published default.
/// The startup restore report remains responsible for warning about quarantined values.
pub fn project(snapshot: &SettingsSnapshot) -> VisualizationSettings {
    if snapshot.schema_version != SETTINGS_SCHEMA_VERSION {
        return VisualizationSettings {
            min_area_px2: TREEMAP_MIN_AREA_PX2_DEFAULT,
            used_default: true,
            warning_key: Some(if snapshot.schema_version > SETTINGS_SCHEMA_VERSION {
                "settings.schema_newer"
            } else {
                "settings.schema_unsupported"
            }),
        };
    }
    let stored = snapshot.values.get(TREEMAP_MIN_AREA_PX2);
    let Some(value) = stored.and_then(valid_area) else {
        return VisualizationSettings {
            min_area_px2: TREEMAP_MIN_AREA_PX2_DEFAULT,
            used_default: true,
            warning_key: stored.map(|_| "settings.value_reset"),
        };
    };
    VisualizationSettings {
        min_area_px2: value,
        used_default: false,
        warning_key: None,
    }
}

fn valid_area(value: &Value) -> Option<u32> {
    let area = u32::try_from(value.as_u64()?).ok()?;
    (TREEMAP_MIN_AREA_PX2_MIN..=TREEMAP_MIN_AREA_PX2_MAX)
        .contains(&area)
        .then_some(area)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{Registry, accepts};
    use du_core_contracts::settings::{
        SettingsChanges, SettingsCommit, SettingsSource, SettingsStore, SettingsStoreError,
    };

    struct Store(SettingsSnapshot);

    impl SettingsStore for Store {
        fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
            Ok(self.0.clone())
        }

        fn commit(
            &mut self,
            expected: u64,
            changes: &SettingsChanges,
            _: SettingsCommit,
        ) -> Result<u64, SettingsStoreError> {
            if self.0.revision != expected {
                return Err(SettingsStoreError::Conflict);
            }
            for (key, value) in changes {
                match value {
                    Some(value) => {
                        self.0.values.insert(key.clone(), value.clone());
                    }
                    None => {
                        self.0.values.remove(key);
                    }
                }
            }
            self.0.revision += 1;
            Ok(self.0.revision)
        }
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
    fn setting_review_commit_and_reset_freeze_values_per_snapshot() -> Result<(), String> {
        let registry = Registry::new(descriptors()).map_err(|error| format!("{error:?}"))?;
        let mut service = crate::settings::SettingsService::open(
            registry,
            Store(SettingsSnapshot {
                schema_version: du_core_contracts::settings::SETTINGS_SCHEMA_VERSION,
                ..SettingsSnapshot::default()
            }),
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
            assert_eq!(service.visualization_settings().min_area_px2, area);
            assert_eq!(service.visualization_settings().used_default, area == 16);
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
        assert_eq!(service.visualization_settings().min_area_px2, 16);
        assert!(service.visualization_settings().used_default);
        assert_eq!(service.visualization_settings().warning_key, None);
        assert!(!service.snapshot().values.contains_key(TREEMAP_MIN_AREA_PX2));
        assert_eq!(project(&initial).min_area_px2, 16);
        Ok(())
    }

    #[test]
    fn restore_status_distinguishes_quarantined_value_from_quarantine_failure() -> Result<(), String>
    {
        let invalid = SettingsSnapshot {
            schema_version: SETTINGS_SCHEMA_VERSION,
            values: BTreeMap::from([(TREEMAP_MIN_AREA_PX2.into(), json!(3))]),
            ..SettingsSnapshot::default()
        };
        let registry = Registry::new(descriptors()).map_err(|error| format!("{error:?}"))?;
        let service = crate::settings::SettingsService::restore(
            registry,
            Store(invalid.clone()),
            |store| {
                Ok((
                    store.0.revision,
                    store.0.schema_version,
                    store
                        .0
                        .values
                        .iter()
                        .map(|(key, value)| (key.clone(), value.to_string()))
                        .collect(),
                ))
            },
            |_, revision, _| Ok(revision + 1),
        );
        assert_eq!(
            service.visualization_settings().warning_key,
            Some("settings.value_reset")
        );
        assert!(!service.restore_status().write_locked);

        let registry = Registry::new(descriptors()).map_err(|error| format!("{error:?}"))?;
        let service = crate::settings::SettingsService::restore(
            registry,
            Store(invalid),
            |store| {
                Ok((
                    store.0.revision,
                    store.0.schema_version,
                    store
                        .0
                        .values
                        .iter()
                        .map(|(key, value)| (key.clone(), value.to_string()))
                        .collect(),
                ))
            },
            |_, _, _| Err(SettingsStoreError::Storage),
        );
        assert_eq!(
            service.visualization_settings().warning_key,
            Some("settings.restore_failed")
        );
        assert!(service.restore_status().write_locked);
        Ok(())
    }
}
