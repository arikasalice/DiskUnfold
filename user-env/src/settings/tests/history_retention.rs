use super::*;
use crate::history_retention::{self, *};
use du_core_contracts::{
    ErrorCode, ErrorEnvelope, Id,
    settings::{
        SETTINGS_SCHEMA_VERSION, SettingGroup, SettingsChanges, SettingsCommit, SettingsSnapshot,
        SettingsStore, SettingsStoreError,
    },
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone)]
struct Memory(SettingsSnapshot);

impl SettingsStore for Memory {
    fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
        Ok(self.0.clone())
    }

    fn commit(
        &mut self,
        expected: u64,
        changes: &SettingsChanges,
        _: SettingsCommit,
    ) -> Result<u64, SettingsStoreError> {
        if expected != self.0.revision {
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

fn service(
    values: BTreeMap<String, Value>,
) -> Result<crate::settings::SettingsService<Memory>, String> {
    let registry = crate::Registry::new(descriptors()).map_err(|error| format!("{error:?}"))?;
    crate::settings::SettingsService::open(
        registry,
        Memory(SettingsSnapshot {
            schema_version: SETTINGS_SCHEMA_VERSION,
            values,
            ..SettingsSnapshot::default()
        }),
    )
    .map_err(|error| format!("{error:?}"))
}

#[test]
fn descriptor_contracts_and_registry_null_default() -> Result<(), String> {
    let [enabled, days] = descriptors();
    assert_eq!(
        (enabled.key.as_str(), enabled.owner.as_str()),
        (ENABLED_KEY, "F21")
    );
    assert_eq!((days.key.as_str(), days.owner.as_str()), (DAYS_KEY, "F21"));
    for descriptor in [&enabled, &days] {
        assert!(descriptor.presentation.hidden);
        assert_eq!(
            descriptor
                .presentation
                .placement
                .map(|placement| placement.group),
            Some(SettingGroup::HistoryPrivacy)
        );
        assert!(!descriptor.editable);
    }
    assert_eq!(enabled.default, json!(false));
    assert_eq!(days.default, Value::Null);
    crate::Registry::new([enabled, days]).map_err(|error| format!("{error:?}"))?;
    let mut boolean_with_null_default = descriptors()[0].clone();
    boolean_with_null_default.default = Value::Null;
    assert!(crate::Registry::new([boolean_with_null_default]).is_err());
    Ok(())
}

#[test]
fn empty_and_stored_policy_values_are_projected_without_storing_the_suggestion()
-> Result<(), String> {
    let empty = service(BTreeMap::new())?;
    let empty_policy = empty.history_retention_policy();
    assert!(!empty_policy.enabled);
    assert_eq!(empty_policy.specified_days, None);
    assert_eq!(empty_policy.suggested_days, 90);
    assert_eq!(empty_policy.unavailable_reason, None);
    assert_eq!(
        empty_policy.policy_fingerprint,
        expected_fingerprint("ok", false, None)
    );

    let stored = service(BTreeMap::from([
        (ENABLED_KEY.into(), json!(true)),
        (DAYS_KEY.into(), json!(180)),
    ]))?;
    let stored_policy = stored.history_retention_policy();
    assert!(stored_policy.enabled);
    assert_eq!(stored_policy.specified_days, Some(180));
    assert_eq!(stored_policy.suggested_days, 90);
    assert_eq!(stored_policy.unavailable_reason, None);

    let max_days =
        service(BTreeMap::from([(DAYS_KEY.into(), json!(u32::MAX))]))?.history_retention_policy();
    assert_eq!(max_days.specified_days, Some(u32::MAX));

    let imported = SettingsSnapshot {
        values: BTreeMap::from([("history.retention.suggested_days".into(), json!(14))]),
        ..SettingsSnapshot::default()
    };
    assert_eq!(
        history_retention::policy(&imported, &[], false).suggested_days,
        90
    );
    Ok(())
}

#[test]
fn days_validation_rejects_invalid_values_and_accepts_limits() -> Result<(), String> {
    for value in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("180"),
        Value::Null,
        json!(u64::from(u32::MAX) + 1),
    ] {
        assert_eq!(
            history_retention::validate_days(&value),
            Err(crate::settings::SettingsError::InvalidValue)
        );
    }
    assert_eq!(history_retention::validate_days(&json!(1)), Ok(1));
    assert_eq!(
        history_retention::validate_days(&json!(u32::MAX)),
        Ok(u32::MAX)
    );
    let invalid_reason =
        ErrorEnvelope::from_reason("settings.value_invalid", Id::from_v7_bytes([1; 16]), vec![])
            .ok_or("settings.value_invalid has no public error mapping")?;
    assert_eq!(invalid_reason.code, ErrorCode::InvalidArgument);
    Ok(())
}

#[test]
fn non_editable_days_reviews_leave_the_snapshot_unchanged() -> Result<(), String> {
    let stored = BTreeMap::from([
        (ENABLED_KEY.into(), json!(true)),
        (DAYS_KEY.into(), json!(180)),
    ]);
    let service = service(stored)?;
    let before = service.snapshot().clone();
    for value in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("180"),
        Value::Null,
        json!(u64::from(u32::MAX) + 1),
    ] {
        let changes = SettingsChanges::from([(DAYS_KEY.to_owned(), Some(value))]);
        assert_eq!(
            service.review(before.revision, &changes),
            Err(crate::settings::SettingsError::NotEditable)
        );
        assert_eq!(service.snapshot(), &before);
    }
    Ok(())
}

#[test]
fn invalid_restored_values_and_read_failures_fail_closed() -> Result<(), String> {
    let registry = crate::Registry::new(descriptors()).map_err(|error| format!("{error:?}"))?;
    let corrupt = crate::settings::SettingsService::restore(
        registry,
        Memory(SettingsSnapshot::default()),
        |_| {
            Ok((
                0,
                SETTINGS_SCHEMA_VERSION,
                BTreeMap::from([
                    (ENABLED_KEY.into(), "true".into()),
                    (DAYS_KEY.into(), "null".into()),
                ]),
            ))
        },
        |_, _, _| Ok(1),
    );
    let invalid = corrupt.history_retention_policy();
    assert!(!invalid.enabled);
    assert_eq!(invalid.specified_days, None);
    assert_eq!(
        invalid.unavailable_reason.as_deref(),
        Some("history.policy_invalid")
    );

    let registry = crate::Registry::new(descriptors()).map_err(|error| format!("{error:?}"))?;
    let unreadable = crate::settings::SettingsService::restore(
        registry,
        Memory(SettingsSnapshot::default()),
        |_| Err(SettingsStoreError::Storage),
        |_, _, _| Ok(1),
    );
    let unreadable = unreadable.history_retention_policy();
    assert!(!unreadable.enabled);
    assert_eq!(unreadable.specified_days, None);
    assert_eq!(
        unreadable.unavailable_reason.as_deref(),
        Some("history.policy_unreadable")
    );
    assert_ne!(invalid.policy_fingerprint, unreadable.policy_fingerprint);
    Ok(())
}

#[test]
fn fingerprints_are_stable_and_cover_values_and_read_state() -> Result<(), String> {
    let base = service(BTreeMap::new())?.history_retention_policy();
    let repeated = service(BTreeMap::new())?.history_retention_policy();
    assert_eq!(base.policy_fingerprint, repeated.policy_fingerprint);

    let enabled =
        service(BTreeMap::from([(ENABLED_KEY.into(), json!(true))]))?.history_retention_policy();
    let days = service(BTreeMap::from([(DAYS_KEY.into(), json!(180))]))?.history_retention_policy();
    let invalid =
        history_retention::policy(&SettingsSnapshot::default(), &[DAYS_KEY.into()], false);
    assert_ne!(base.policy_fingerprint, enabled.policy_fingerprint);
    assert_ne!(base.policy_fingerprint, days.policy_fingerprint);
    assert_ne!(base.policy_fingerprint, invalid.policy_fingerprint);
    assert_ne!(
        base.policy_fingerprint,
        history_retention::unreadable_policy().policy_fingerprint
    );
    Ok(())
}

#[test]
fn older_registry_preserves_unknown_retention_rows_through_restore() -> Result<(), String> {
    let old_registry =
        crate::Registry::new(Vec::<Descriptor>::new()).map_err(|error| format!("{error:?}"))?;
    let service = crate::settings::SettingsService::restore(
        old_registry,
        Memory(SettingsSnapshot::default()),
        |_| {
            Ok((
                7,
                SETTINGS_SCHEMA_VERSION,
                BTreeMap::from([
                    (ENABLED_KEY.into(), "true".into()),
                    (DAYS_KEY.into(), "180".into()),
                ]),
            ))
        },
        |_, _, _| Err(SettingsStoreError::Storage),
    );
    assert!(service.restore_status().invalid_keys.is_empty());
    assert_eq!(
        service.snapshot().values.get(ENABLED_KEY),
        Some(&json!(true))
    );
    assert_eq!(service.snapshot().values.get(DAYS_KEY), Some(&json!(180)));
    Ok(())
}

fn expected_fingerprint(state: &str, enabled: bool, days: Option<u32>) -> String {
    let days = days.map_or_else(|| "null".to_owned(), |days| days.to_string());
    let canonical = format!("{{\"days\":{days},\"enabled\":{enabled},\"state\":\"{state}\"}}");
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"diskunfold.history.retention.v1\0");
    hasher.update(canonical.as_bytes());
    hasher.finalize().as_bytes()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
