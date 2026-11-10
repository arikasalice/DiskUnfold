use super::*;
use crate::space::{SHARED_TABLE_MAX_ENTRIES, descriptors};
use du_core_contracts::settings::SettingsSource;
use serde_json::json;

struct Store(Result<SettingsSnapshot, SettingsStoreError>);
impl SettingsStore for Store {
    fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
        self.0.clone()
    }
    fn commit(
        &mut self,
        _: u64,
        _: &SettingsChanges,
        _: SettingsCommit,
    ) -> Result<u64, SettingsStoreError> {
        Ok(2)
    }
}
fn registry() -> Result<Registry, String> {
    let mut all = descriptors();
    all.push(crate::Descriptor {
        key: "good".into(),
        owner: "test".into(),
        default: json!(false),
        kind: crate::ValueKind::Boolean,
        editable: true,
    });
    Registry::new(all).map_err(|e| format!("{e:?}"))
}
fn snapshot(version: u32) -> SettingsSnapshot {
    SettingsSnapshot {
        revision: 1,
        schema_version: version,
        values: [
            (SHARED_TABLE_MAX_ENTRIES.into(), json!(1)),
            ("future.key".into(), json!("preserve")),
            ("good".into(), json!(true)),
        ]
        .into(),
    }
}
fn load(
    store: &mut Store,
) -> Result<(u64, u32, std::collections::BTreeMap<String, String>), SettingsStoreError> {
    let snapshot = store.load()?;
    Ok((
        snapshot.revision,
        snapshot.schema_version,
        snapshot
            .values
            .into_iter()
            .map(|(k, v)| (k, v.to_string()))
            .collect(),
    ))
}
#[test]
fn invalid_known_value_only_is_quarantined_and_unknown_retained() -> Result<(), String> {
    let service = SettingsService::restore(
        registry()?,
        Store(Ok(snapshot(1))),
        load,
        |_, revision, invalid| {
            assert_eq!(revision, 1);
            assert_eq!(invalid.len(), 1);
            assert_eq!(
                invalid.get(SHARED_TABLE_MAX_ENTRIES).map(String::as_str),
                Some("1")
            );
            Ok(2)
        },
    );
    assert_eq!(
        service.restore_status().invalid_keys,
        vec![SHARED_TABLE_MAX_ENTRIES]
    );
    assert_eq!(service.snapshot().revision, 2);
    let status = service.restore_status();
    assert_eq!(
        (
            status.stored_revision,
            status.stored_schema,
            status.app_schema,
            status.migrated
        ),
        (Some(1), Some(1), 1, false)
    );
    assert_eq!(
        status.items.get("good"),
        Some(&RestoreItem {
            source: RestoreSource::Stored,
            reason: None
        })
    );
    assert_eq!(
        status.items.get(SHARED_TABLE_MAX_ENTRIES),
        Some(&RestoreItem {
            source: RestoreSource::Default,
            reason: Some("settings.value_invalid")
        })
    );
    assert!(!status.items.contains_key("future.key"));
    assert_eq!(
        service.snapshot().values.get("future.key"),
        Some(&json!("preserve"))
    );
    assert_eq!(service.snapshot().values.get("good"), Some(&json!(true)));
    assert_eq!(
        service
            .registry
            .catalog(service.snapshot())
            .into_iter()
            .find(|item| item.descriptor.key == SHARED_TABLE_MAX_ENTRIES)
            .ok_or("missing setting")?
            .value,
        &json!(1_000_000)
    );
    Ok(())
}
#[test]
fn failures_and_newer_schema_default_and_lock_without_quarantine() -> Result<(), String> {
    for input in [Ok(snapshot(2)), Err(SettingsStoreError::Storage)] {
        let called = std::cell::Cell::new(false);
        let service = SettingsService::restore(registry()?, Store(input), load, |_, _, _| {
            called.set(true);
            Err(SettingsStoreError::Storage)
        });
        assert!(!called.get());
        assert!(service.restore_status().write_locked);
        assert!(
            service
                .restore_status()
                .items
                .values()
                .all(|item| item.source == RestoreSource::Default
                    && item.reason == service.restore_status().failure_reason)
        );
        assert!(service.snapshot().values.is_empty());
        assert_eq!(
            service.review(0, &SettingsChanges::new()),
            Err(SettingsError::RestoreLocked)
        );
    }
    let mut service =
        SettingsService::restore(registry()?, Store(Ok(snapshot(1))), load, |_, _, _| {
            Err(SettingsStoreError::Conflict)
        });
    assert_eq!(
        service.restore_status().failure_reason,
        Some("settings.restore_failed")
    );
    assert_eq!(
        service.apply(
            0,
            &SettingsChanges::new(),
            [0; 32],
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 0
            }
        ),
        Err(SettingsError::RestoreLocked)
    );
    Ok(())
}

#[test]
fn absent_value_is_a_default_without_failure_reason() -> Result<(), String> {
    let empty = SettingsSnapshot {
        schema_version: 1,
        ..SettingsSnapshot::default()
    };
    let service = SettingsService::restore(registry()?, Store(Ok(empty)), load, |_, _, _| {
        Err(SettingsStoreError::Storage)
    });
    assert_eq!(
        service.restore_status().items.get(SHARED_TABLE_MAX_ENTRIES),
        Some(&RestoreItem {
            source: RestoreSource::Default,
            reason: None
        })
    );
    Ok(())
}

#[test]
fn malformed_known_raw_value_is_isolated_but_unknown_raw_value_is_not() -> Result<(), String> {
    let service = SettingsService::restore(
        registry()?,
        Store(Ok(snapshot(1))),
        |_| {
            Ok((
                1,
                1,
                [
                    (SHARED_TABLE_MAX_ENTRIES.into(), "{broken".into()),
                    ("future.key".into(), "{unknown".into()),
                ]
                .into(),
            ))
        },
        |_, _, bad| {
            assert_eq!(bad.len(), 1);
            assert_eq!(
                bad.get(SHARED_TABLE_MAX_ENTRIES).map(String::as_str),
                Some("{broken")
            );
            Ok(2)
        },
    );
    assert_eq!(
        service.restore_status().invalid_keys,
        vec![SHARED_TABLE_MAX_ENTRIES]
    );
    assert!(!service.restore_status().write_locked);
    Ok(())
}
