//! F21 history retention settings and their safe read projection.

use du_core_contracts::{
    history_retention_ipc::HistoryRetentionPolicy,
    settings::{SettingGroup, SettingLevel, SettingPresentation, SettingsSnapshot},
};
use serde_json::{Value, json};

use crate::{Descriptor, ValueKind};

/// Whether automatic history retention is enabled.
pub const ENABLED_KEY: &str = "history.retention.enabled";
/// Optional user-selected history age limit.
pub const DAYS_KEY: &str = "history.retention.days";
/// Built-in suggestion; it is deliberately not a registered setting.
pub const SUGGESTED_DAYS: u32 = 90;

const MAX_DAYS: i64 = u32::MAX as i64;
const HASH_DOMAIN: &[u8] = b"diskunfold.history.retention.v1\0";

/// The hidden F21 descriptors stored in the existing settings table.
pub fn descriptors() -> [Descriptor; 2] {
    [
        Descriptor {
            presentation: hidden_history_presentation(),
            key: ENABLED_KEY.into(),
            owner: "F21".into(),
            default: json!(false),
            kind: ValueKind::Boolean,
            editable: false,
        },
        Descriptor {
            presentation: hidden_history_presentation(),
            key: DAYS_KEY.into(),
            owner: "F21".into(),
            default: Value::Null,
            kind: ValueKind::OptionalInteger {
                min: 1,
                max: MAX_DAYS,
            },
            editable: false,
        },
    ]
}

fn hidden_history_presentation() -> SettingPresentation {
    SettingPresentation::hidden()
        .with_placement(SettingGroup::HistoryPrivacy, SettingLevel::Detailed)
}

/// Projects one settings snapshot and restore report into a fail-closed policy.
///
/// `policy_fingerprint` is lowercase hex for the first 16 BLAKE3 bytes of
/// `diskunfold.history.retention.v1\0` plus canonical UTF-8 JSON with sorted
/// keys: `{"days":<n>|null,"enabled":<bool>,"state":"<ok|unreadable|invalid>"}`.
/// Integers use plain decimal notation and the JSON contains no spaces.
pub fn policy(
    snapshot: &SettingsSnapshot,
    invalid_keys: &[String],
    unreadable: bool,
) -> HistoryRetentionPolicy {
    if invalid_keys
        .iter()
        .any(|key| key == ENABLED_KEY || key == DAYS_KEY)
    {
        return unavailable("invalid");
    }
    if unreadable {
        return unavailable("unreadable");
    }

    let enabled = match snapshot.values.get(ENABLED_KEY) {
        Some(value) => match value.as_bool() {
            Some(enabled) => enabled,
            None => return unavailable("invalid"),
        },
        None => false,
    };
    let specified_days = match snapshot.values.get(DAYS_KEY) {
        Some(value) => match validate_days(value) {
            Ok(days) => Some(days),
            Err(_) => return unavailable("invalid"),
        },
        None => None,
    };
    available(enabled, specified_days)
}

/// Validates that a history retention age is an integer from 1 through `u32::MAX`.
///
/// Returns [`crate::settings_error::SettingsError::InvalidValue`] for any other JSON value.
pub fn validate_days(value: &Value) -> Result<u32, crate::settings_error::SettingsError> {
    value
        .as_i64()
        .filter(|days| (1..=MAX_DAYS).contains(days))
        .and_then(|days| u32::try_from(days).ok())
        .ok_or(crate::settings_error::SettingsError::InvalidValue)
}

/// Returns the fail-closed policy used when the settings service cannot be read.
pub fn unreadable_policy() -> HistoryRetentionPolicy {
    unavailable("unreadable")
}

fn available(enabled: bool, specified_days: Option<u32>) -> HistoryRetentionPolicy {
    HistoryRetentionPolicy {
        enabled,
        specified_days,
        suggested_days: SUGGESTED_DAYS,
        policy_fingerprint: fingerprint("ok", enabled, specified_days),
        unavailable_reason: None,
    }
}

fn unavailable(state: &'static str) -> HistoryRetentionPolicy {
    HistoryRetentionPolicy {
        enabled: false,
        specified_days: None,
        suggested_days: SUGGESTED_DAYS,
        policy_fingerprint: fingerprint(state, false, None),
        unavailable_reason: Some(format!("history.policy_{state}")),
    }
}

/// Hashes the versioned canonical policy record; `None` remains distinct from a stored value.
///
/// The digest is lowercase hex for the first 16 BLAKE3 bytes of
/// `diskunfold.history.retention.v1\0` followed by canonical UTF-8 JSON with
/// sorted keys: `{"days":<n>|null,"enabled":<bool>,"state":"<state>"}`.
/// Integers use plain decimal notation and the state is `ok`, `unreadable` or `invalid`.
fn fingerprint(state: &str, enabled: bool, specified_days: Option<u32>) -> String {
    let days = specified_days.map_or_else(|| "null".to_owned(), |days| days.to_string());
    let canonical = format!("{{\"days\":{days},\"enabled\":{enabled},\"state\":\"{state}\"}}");
    let mut hasher = blake3::Hasher::new();
    hasher.update(HASH_DOMAIN);
    hasher.update(canonical.as_bytes());
    let mut output = String::with_capacity(32);
    for byte in &hasher.finalize().as_bytes()[..16] {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
