//! Settings for remembering user-selected analysis scopes.
use crate::{Descriptor, ValueKind};
use du_core_contracts::settings::SettingsSnapshot;
use serde_json::json;

/// The F01 setting controlling future recent-scope writes.
pub const REMEMBER_RECENT: &str = "scope.remember_recent";

/// Registers the published default; this does not write or erase recent scopes.
pub fn descriptors() -> Vec<Descriptor> {
    vec![Descriptor {
        presentation: du_core_contracts::settings::SettingPresentation::for_setting(
            REMEMBER_RECENT,
        )
        .with_placement(
            du_core_contracts::settings::SettingGroup::HistoryPrivacy,
            du_core_contracts::settings::SettingLevel::Basic,
        ),
        key: REMEMBER_RECENT.into(),
        owner: "F01".into(),
        default: json!(true),
        kind: ValueKind::Boolean,
        editable: true,
    }]
}

/// Resolves the setting from a restored snapshot, defaulting on unreadable data.
///
/// A future recent-scope writer must call this for each write admission; this
/// function does not itself persist a scope or authorize a filesystem action.
#[must_use]
pub fn remember_recent(snapshot: &SettingsSnapshot) -> bool {
    if snapshot.schema_version != du_core_contracts::settings::SETTINGS_SCHEMA_VERSION {
        return true;
    }
    snapshot
        .values
        .get(REMEMBER_RECENT)
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_disabled_and_unreadable_values_follow_the_published_policy() {
        let mut snapshot = SettingsSnapshot {
            schema_version: 2,
            ..SettingsSnapshot::default()
        };
        assert!(remember_recent(&snapshot));
        snapshot.values.insert(REMEMBER_RECENT.into(), json!(false));
        assert!(!remember_recent(&snapshot));
        snapshot
            .values
            .insert(REMEMBER_RECENT.into(), json!("invalid"));
        assert!(remember_recent(&snapshot));
        snapshot.values.insert(REMEMBER_RECENT.into(), json!(false));
        snapshot.schema_version = 3;
        assert!(remember_recent(&snapshot));
    }
}
