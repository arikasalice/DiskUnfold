//! F15 startup memories kept across launches (#195).
//!
//! Each memory is one state key: the startup notice the user dismissed and
//! the Support information the user has seen. Both are hidden settings, so no
//! settings screen or search lists them. The default `""` means nothing is
//! remembered; a stored value that fails the grammar is quarantined when the
//! settings are restored and reads as the default.
//!
//! The memories are app state, not preferences, so the F14 settings paths
//! that 0.0.1 does not have yet must keep these rules (#205):
//! - An export leaves out every key for which [`is_memory`] holds.
//! - A reset to defaults keeps the memories; it does not show a dismissed
//!   startup notice again.
//! - Any path that changes a memory (an import, a reset, a CLI write) calls
//!   `StartupAccessSession::load_memories` in app-host with the stored values
//!   after its commit, so `app.startup_status` answers from them at once.
use crate::{Descriptor, ValueKind};
use du_core_contracts::{
    settings::{SettingPresentation, SettingsSnapshot},
    startup_ipc::{STARTUP_STATE_KEY_CHARSET, STARTUP_STATE_KEY_MAX_BYTES},
};
use serde_json::json;

/// The startup notice state key the user dismissed.
pub const NOTICE_DISMISSED: &str = "startup.notice_dismissed";
/// The state key of the Support information the user has seen.
pub const REVIEW_SEEN: &str = "startup.review_seen";

/// Whether `key` names a startup memory, which settings export leaves out.
pub fn is_memory(key: &str) -> bool {
    key.starts_with("startup.")
}

/// Whether `key` follows the startup state key grammar `[a-z0-9_.:|]`, at most
/// 200 bytes. The empty string, which clears a memory, is allowed.
pub fn is_state_key(key: &str) -> bool {
    key.len() <= STARTUP_STATE_KEY_MAX_BYTES
        && key.chars().all(|c| STARTUP_STATE_KEY_CHARSET.contains(c))
}

/// The hidden descriptors of [`NOTICE_DISMISSED`] and [`REVIEW_SEEN`].
pub fn descriptors() -> [Descriptor; 2] {
    [NOTICE_DISMISSED, REVIEW_SEEN].map(|key| Descriptor {
        presentation: SettingPresentation::hidden(),
        key: key.into(),
        owner: "F15".into(),
        default: json!(""),
        kind: ValueKind::Text {
            max_bytes: STARTUP_STATE_KEY_MAX_BYTES,
            charset: STARTUP_STATE_KEY_CHARSET,
        },
        editable: true,
    })
}

/// The state key remembered under `key`, or `None` when nothing is.
pub fn remembered(snapshot: &SettingsSnapshot, key: &str) -> Option<String> {
    snapshot
        .values
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && is_state_key(value))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Registry, settings::SettingsService};
    use du_core_contracts::settings::{
        SETTINGS_SCHEMA_VERSION, SettingsChanges, SettingsCommit, SettingsStore, SettingsStoreError,
    };
    use std::collections::BTreeMap;

    type Raw = BTreeMap<String, String>;

    /// Stored values as JSON text, as the database keeps them.
    struct Store(Raw);
    impl SettingsStore for Store {
        fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
            Err(SettingsStoreError::Storage)
        }
        fn commit(
            &mut self,
            revision: u64,
            _: &SettingsChanges,
            _: SettingsCommit,
        ) -> Result<u64, SettingsStoreError> {
            Ok(revision + 1)
        }
    }

    /// Restores `stored` with `registry` and returns the service and the
    /// keys that were quarantined.
    fn restore(registry: Registry, stored: Raw) -> (SettingsService<Store>, Vec<String>) {
        let mut quarantined = Vec::new();
        let service = SettingsService::restore(
            registry,
            Store(stored),
            |store| Ok((1, SETTINGS_SCHEMA_VERSION, store.0.clone())),
            |_, revision, invalid| {
                quarantined.extend(invalid.keys().cloned());
                Ok(revision + 1)
            },
        );
        (service, quarantined)
    }

    fn builtin(with_memories: bool) -> Result<Registry, String> {
        let descriptors = crate::settings::builtin_descriptors(&["en", "ko"])
            .map_err(|error| format!("{error:?}"))?
            .into_iter()
            .filter(|descriptor| with_memories || !is_memory(&descriptor.key));
        Registry::new(descriptors).map_err(|error| format!("{error:?}"))
    }

    #[test]
    fn only_the_hidden_startup_memories_are_memories() -> Result<(), String> {
        let all = crate::settings::builtin_descriptors(&["en", "ko"])
            .map_err(|error| format!("{error:?}"))?;
        let memories: Vec<_> = all
            .iter()
            .filter(|descriptor| is_memory(&descriptor.key))
            .collect();
        let mut keys: Vec<_> = memories.iter().map(|d| d.key.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(keys, [NOTICE_DISMISSED, REVIEW_SEEN]);
        // An export that drops memories therefore drops only hidden settings.
        assert!(memories.iter().all(|d| d.presentation.hidden));
        Ok(())
    }

    #[test]
    fn an_older_build_keeps_the_memories_it_does_not_know() -> Result<(), String> {
        let stored = Raw::from([(REVIEW_SEEN.into(), "\"grade:limited\"".into())]);
        let (service, quarantined) = restore(builtin(false)?, stored);
        assert!(quarantined.is_empty());
        assert_eq!(
            service.snapshot().values.get(REVIEW_SEEN),
            Some(&json!("grade:limited"))
        );
        Ok(())
    }

    #[test]
    fn an_invalid_stored_memory_is_quarantined_and_remembers_nothing() -> Result<(), String> {
        let stored = Raw::from([
            (NOTICE_DISMISSED.into(), "\"Bad Key\"".into()),
            (REVIEW_SEEN.into(), "\"grade:limited\"".into()),
        ]);
        let (service, quarantined) = restore(builtin(true)?, stored);
        assert_eq!(quarantined, [NOTICE_DISMISSED]);
        assert_eq!(remembered(service.snapshot(), NOTICE_DISMISSED), None);
        assert_eq!(
            remembered(service.snapshot(), REVIEW_SEEN).as_deref(),
            Some("grade:limited")
        );
        Ok(())
    }

    #[test]
    fn both_memories_are_hidden_f15_text_settings_that_default_to_nothing() {
        for descriptor in descriptors() {
            assert!(descriptor.presentation.hidden, "{}", descriptor.key);
            assert_eq!(descriptor.owner, "F15");
            assert_eq!(descriptor.default, json!(""));
            assert!(descriptor.editable);
            assert!(crate::accepts(&descriptor.kind, &descriptor.default));
        }
    }

    #[test]
    fn the_text_kind_takes_only_grammar_strings_up_to_the_limit() {
        let [descriptor, _] = descriptors();
        let accepts = |value| crate::accepts(&descriptor.kind, &value);
        assert!(accepts(json!(
            "grade:unverifiable|support_matrix:env_unknown"
        )));
        assert!(accepts(json!("a".repeat(200))));
        assert!(!accepts(json!("a".repeat(201))));
        assert!(!accepts(json!("Grade")));
        assert!(!accepts(json!("grade unverifiable")));
        assert!(!accepts(json!(1)));
        assert!(!accepts(json!(null)));
        assert!(is_state_key(""));
        for bad in ["grade-x", "é", "a/b"] {
            assert!(!is_state_key(bad), "{bad}");
            assert!(!accepts(json!(bad)), "{bad}");
        }
    }

    #[test]
    fn a_missing_or_empty_value_remembers_nothing() {
        let snapshot = |value: serde_json::Value| SettingsSnapshot {
            values: BTreeMap::from([(NOTICE_DISMISSED.to_owned(), value)]),
            ..SettingsSnapshot::default()
        };
        assert_eq!(
            remembered(&SettingsSnapshot::default(), NOTICE_DISMISSED),
            None
        );
        assert_eq!(remembered(&snapshot(json!("")), NOTICE_DISMISSED), None);
        assert_eq!(
            remembered(&snapshot(json!("Bad Key")), NOTICE_DISMISSED),
            None
        );
        assert_eq!(
            remembered(&snapshot(json!("grade:limited")), NOTICE_DISMISSED).as_deref(),
            Some("grade:limited")
        );
    }
}
