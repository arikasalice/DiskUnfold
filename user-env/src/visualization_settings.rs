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

/// Returns the published F06 descriptor for explicit registry composition.
///
/// It is deliberately excluded from [`crate::settings::builtin_descriptors`] until the
/// application composes both the settings catalog and the visualization consumer.
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

/// Projects raw values from a settings snapshot, falling back to the published default.
///
/// This helper is crate-private because it validates schema and value shape, but cannot verify
/// that the snapshot was restored under the published F06 descriptor. Consumers must use the
/// descriptor-checked [`crate::settings::SettingsService::visualization_settings`] accessor.
pub(crate) fn project(snapshot: &SettingsSnapshot) -> VisualizationSettings {
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
mod tests;
