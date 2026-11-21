//! The hidden size aggregation rule setting (F03 FR14.HD2.app-host.DD4).
use super::*;
use crate::settings::{SettingsError, SettingsService};
use crate::{Registry, i18n::embedded};
use du_core_contracts::settings::{
    SETTINGS_SCHEMA_VERSION, SettingsChanges, SettingsCommit, SettingsSnapshot, SettingsSource,
    SettingsStore, SettingsStoreError,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const OTHER: &str = "size.agg.other";

/// Commits into memory and reports the next revision.
struct Memory(SettingsSnapshot);

impl SettingsStore for Memory {
    fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
        Ok(self.0.clone())
    }
    fn commit(
        &mut self,
        expected: u64,
        _: &SettingsChanges,
        _: SettingsCommit,
    ) -> Result<u64, SettingsStoreError> {
        Ok(expected + 1)
    }
}

fn ids() -> Vec<String> {
    vec![DEFAULT_SIZE_RULE_ID.to_owned(), OTHER.to_owned()]
}

fn service() -> Result<SettingsService<Memory>, Box<dyn std::error::Error>> {
    let registry = Registry::new([aggregation_rule_descriptor(&ids())])
        .map_err(|error| format!("{error:?}"))?;
    let store = Memory(SettingsSnapshot {
        schema_version: SETTINGS_SCHEMA_VERSION,
        ..SettingsSnapshot::default()
    });
    Ok(SettingsService::open(registry, store).map_err(|error| format!("{error:?}"))?)
}

fn commit() -> SettingsCommit {
    SettingsCommit {
        source: SettingsSource::Owner,
        timestamp_ms: 1,
    }
}

fn set(id: &str) -> SettingsChanges {
    SettingsChanges::from([(AGGREGATION_RULE.into(), Some(json!(id)))])
}

#[test]
fn descriptor_is_declared_with_the_default_rule_and_no_wording() {
    let descriptor = aggregation_rule_descriptor(&ids());
    assert_eq!(descriptor.key, "space.aggregation_rule");
    assert_eq!(descriptor.owner, "F03");
    assert_eq!(descriptor.default, json!("size.agg.subtree-unique"));
    assert!(descriptor.editable);
    assert_eq!(
        descriptor.kind,
        ValueKind::Choice(vec![json!(DEFAULT_SIZE_RULE_ID), json!(OTHER)])
    );
    let presentation = descriptor.presentation;
    assert!(presentation.hidden);
    assert!(presentation.name_key.is_empty() && presentation.description_key.is_empty());
    assert_eq!(presentation.placement, None);
}

#[test]
fn registration_requires_the_default_rule_among_the_registered_ids() {
    let descriptor = aggregation_rule_descriptor(&[OTHER.to_owned()]);
    assert!(Registry::new([descriptor]).is_err());
}

#[test]
fn builtin_catalog_declares_the_setting_but_no_listing_shows_it() -> TestResult {
    let descriptors = crate::settings::builtin_descriptors(&["en", "ko"])
        .map_err(|error| format!("{error:?}"))?;
    let hidden: Vec<_> = descriptors
        .iter()
        .filter(|descriptor| descriptor.presentation.hidden)
        .map(|descriptor| descriptor.key.as_str())
        .collect();
    assert_eq!(
        hidden,
        [
            AGGREGATION_RULE,
            crate::startup_memory::NOTICE_DISMISSED,
            crate::startup_memory::REVIEW_SEEN,
            crate::history_retention::ENABLED_KEY,
            crate::history_retention::DAYS_KEY
        ]
    );

    let registry = Registry::new(descriptors).map_err(|error| format!("{error:?}"))?;
    let snapshot = SettingsSnapshot {
        revision: 3,
        schema_version: SETTINGS_SCHEMA_VERSION,
        ..SettingsSnapshot::default()
    };
    // The catalog still reads the value, so a consumer can resolve the default.
    let rows = registry.catalog(&snapshot);
    let row = rows
        .iter()
        .find(|row| row.descriptor.key == AGGREGATION_RULE)
        .ok_or("the setting is not declared")?;
    assert!(row.uses_default);
    assert_eq!(row.value, &json!(DEFAULT_SIZE_RULE_ID));

    // Search and the restoration report never list it.
    let resources = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    for query in [AGGREGATION_RULE, "aggregation", "size.agg"] {
        let result = registry
            .search(&snapshot, &resources, query)
            .map_err(|error| format!("{error:?}"))?;
        assert!(
            result
                .hits
                .iter()
                .all(|hit| hit.entry.descriptor.key != AGGREGATION_RULE)
        );
    }
    let service = SettingsService::open(
        registry,
        Memory(SettingsSnapshot {
            revision: 3,
            schema_version: SETTINGS_SCHEMA_VERSION,
            values: [(AGGREGATION_RULE.into(), json!(DEFAULT_SIZE_RULE_ID))].into(),
        }),
    )
    .map_err(|error| format!("{error:?}"))?;
    assert!(
        !service
            .restore_status()
            .items
            .contains_key(AGGREGATION_RULE)
    );
    assert!(
        service
            .restore_status()
            .items
            .contains_key("space.default_basis")
    );
    Ok(())
}

#[test]
fn a_write_accepts_registered_ids_and_rejects_others() -> TestResult {
    let service = service()?;
    assert!(service.review(0, &set(OTHER)).is_ok());
    assert!(service.review(0, &set(DEFAULT_SIZE_RULE_ID)).is_ok());
    for bad in ["size.agg.nope", "", "SIZE.AGG.SUBTREE-UNIQUE"] {
        assert_eq!(
            service.review(0, &set(bad)),
            Err(SettingsError::InvalidValue)
        );
    }
    let not_a_string = SettingsChanges::from([(AGGREGATION_RULE.into(), Some(json!(1)))]);
    assert_eq!(
        service.review(0, &not_a_string),
        Err(SettingsError::InvalidValue)
    );
    Ok(())
}

#[test]
fn a_stored_id_round_trips_and_the_default_is_not_persisted() -> TestResult {
    let mut service = service()?;
    let changes = set(OTHER);
    let reviewed = service
        .review(0, &changes)
        .map_err(|error| format!("{error:?}"))?;
    service
        .apply(0, &changes, reviewed, commit())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        service.snapshot().values.get(AGGREGATION_RULE),
        Some(&json!(OTHER))
    );

    // Choosing the default again removes the override instead of storing it.
    let changes = set(DEFAULT_SIZE_RULE_ID);
    let reviewed = service
        .review(1, &changes)
        .map_err(|error| format!("{error:?}"))?;
    service
        .apply(1, &changes, reviewed, commit())
        .map_err(|error| format!("{error:?}"))?;
    assert!(!service.snapshot().values.contains_key(AGGREGATION_RULE));
    Ok(())
}

fn restore(raw: &str, ids: &[String]) -> Result<SettingsService<Memory>, String> {
    let registry =
        Registry::new([aggregation_rule_descriptor(ids)]).map_err(|error| format!("{error:?}"))?;
    let raw = raw.to_owned();
    Ok(SettingsService::restore(
        registry,
        Memory(SettingsSnapshot::default()),
        move |_| {
            Ok((
                4,
                SETTINGS_SCHEMA_VERSION,
                [(AGGREGATION_RULE.to_owned(), raw)].into(),
            ))
        },
        |_, revision, invalid| {
            assert_eq!((revision, invalid.len()), (4, 1));
            Ok(5)
        },
    ))
}

#[test]
fn restore_keeps_a_registered_id_and_quarantines_an_unregistered_one() -> TestResult {
    let kept = restore("\"size.agg.other\"", &ids())?;
    assert_eq!(
        kept.snapshot().values.get(AGGREGATION_RULE),
        Some(&json!(OTHER))
    );
    assert!(kept.restore_status().invalid_keys.is_empty());

    // After a downgrade the stored ID may name a rule this build does not register.
    let downgraded = restore("\"size.agg.other\"", &[DEFAULT_SIZE_RULE_ID.to_owned()])?;
    assert!(!downgraded.snapshot().values.contains_key(AGGREGATION_RULE));
    assert_eq!(downgraded.restore_status().invalid_keys, [AGGREGATION_RULE]);
    assert_eq!(downgraded.snapshot().revision, 5);
    Ok(())
}
