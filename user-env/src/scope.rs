//! Settings for remembering user-selected analysis scopes.
use crate::{Descriptor, ValueKind};
use du_core_contracts::settings::SettingsSnapshot;
use serde_json::json;
pub mod recent;

/// The F01 setting controlling future recent-scope writes.
pub const REMEMBER_RECENT: &str = "scope.remember_recent";
/// FR8.HD1.app-host.DD2: retention applied after the next eligible write.
pub const RECENT_LIMIT: &str = "scope.recent_limit";
/// Published recent-scope retention choices, in rows.
pub const RECENT_LIMIT_CHOICES: [usize; 4] = [50, 100, 200, 500];
/// Default recent-scope retention, in rows.
pub const RECENT_LIMIT_DEFAULT: usize = 100;
/// Maximum number of existing recent scopes that can be listed.
pub const RECENT_LIMIT_MAX: usize = 500;

/// Registers F01's recent-scope settings without writing or erasing stored rows.
///
/// FR8.HD1.app-host.DD2 and FR14.HD1.app-host.DD1 define the retention and
/// remember-setting behavior represented by these descriptors.
pub fn descriptors() -> Vec<Descriptor> {
    vec![
        Descriptor {
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
        },
        Descriptor {
            presentation: du_core_contracts::settings::SettingPresentation::for_setting(
                RECENT_LIMIT,
            )
            .with_placement(
                du_core_contracts::settings::SettingGroup::HistoryPrivacy,
                du_core_contracts::settings::SettingLevel::Basic,
            ),
            key: RECENT_LIMIT.into(),
            owner: "F01".into(),
            default: json!(RECENT_LIMIT_DEFAULT),
            kind: ValueKind::Choice(
                RECENT_LIMIT_CHOICES
                    .into_iter()
                    .map(|value| json!(value))
                    .collect(),
            ),
            editable: true,
        },
    ]
}

/// Resolves the setting from a restored snapshot, defaulting on unreadable data.
///
/// A future recent-scope writer must call this for each write admission; this
/// function does not itself persist a scope or authorize a filesystem action.
#[must_use]
pub fn remember_recent(snapshot: &SettingsSnapshot) -> bool {
    let value = if snapshot.schema_version == du_core_contracts::settings::SETTINGS_SCHEMA_VERSION {
        snapshot
            .values
            .get(REMEMBER_RECENT)
            .and_then(|v| v.as_bool())
    } else {
        None
    };
    remember_recent_value(value)
}

/// Resolves the policy for a value read inside the recent-scope write transaction.
#[must_use]
pub fn remember_recent_value(value: Option<bool>) -> bool {
    value.unwrap_or(true)
}

/// Resolves the production write policy from values read in one database transaction.
///
/// FR8.HD1.app-host.DD2 and FR14.HD1.app-host.DD1: absent or invalid settings
/// use the published defaults; a disabled remember setting skips the write.
#[must_use]
pub fn recent_write_limit(
    remember_json: Option<&str>,
    limit_json: Option<&str>,
) -> Option<std::num::NonZeroUsize> {
    let remember = remember_json.and_then(|value| serde_json::from_str::<bool>(value).ok());
    if !remember_recent_value(remember) {
        return None;
    }
    let limit = limit_json
        .and_then(|value| serde_json::from_str::<u64>(value).ok())
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| RECENT_LIMIT_CHOICES.contains(value))
        .unwrap_or(RECENT_LIMIT_DEFAULT);
    std::num::NonZeroUsize::new(limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::settings::SETTINGS_SCHEMA_VERSION;

    #[test]
    fn default_disabled_and_unreadable_values_follow_the_published_policy() {
        let mut snapshot = SettingsSnapshot {
            schema_version: SETTINGS_SCHEMA_VERSION,
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
        snapshot.schema_version = SETTINGS_SCHEMA_VERSION + 1;
        assert!(remember_recent(&snapshot));
    }

    #[test]
    fn recent_limit_is_an_integer_choice_and_defaults_for_unrecognized_data() {
        let settings = descriptors();
        let descriptor = settings
            .iter()
            .find(|descriptor| descriptor.key == RECENT_LIMIT);
        assert_eq!(
            descriptor.map(|value| value.default.clone()),
            Some(json!(100))
        );
        assert_eq!(
            descriptor.map(|value| value.kind.clone()),
            Some(ValueKind::Choice(vec![
                json!(50),
                json!(100),
                json!(200),
                json!(500)
            ]))
        );
        assert_eq!(
            recent_write_limit(Some("true"), Some("50")).map(std::num::NonZeroUsize::get),
            Some(50)
        );
        assert_eq!(
            recent_write_limit(Some("true"), Some("500")).map(std::num::NonZeroUsize::get),
            Some(500)
        );
        assert_eq!(
            recent_write_limit(Some("true"), Some("51")).map(std::num::NonZeroUsize::get),
            Some(100)
        );
        assert_eq!(
            recent_write_limit(Some("invalid"), Some("50.0")).map(std::num::NonZeroUsize::get),
            Some(100)
        );
        assert_eq!(recent_write_limit(Some("false"), Some("500")), None);
        assert_eq!(
            recent_write_limit(None, None).map(std::num::NonZeroUsize::get),
            Some(100)
        );
    }
}
