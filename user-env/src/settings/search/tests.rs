use super::*;
use crate::i18n::embedded;
use du_core_contracts::settings::SettingsSnapshot;

#[test]
fn published_normalization_is_not_compatibility_folding() {
    for (a, b) in [
        ("Straße", "STRASSE"),
        ("Σςσ", "σσσ"),
        ("Café", "Cafe\u{301}"),
        ("한글", "한글"),
    ] {
        assert_eq!(normalize(a), normalize(b));
    }
    assert_ne!(normalize("Ａ"), normalize("A"));
    assert!(normalize("Straße").contains(&normalize("SS")));
}

#[test]
fn builtin_catalog_and_registered_resources_share_one_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let descriptors =
        super::super::builtin_descriptors(&["en", "ko"]).map_err(|e| format!("{e:?}"))?;
    let registry = Registry::new(descriptors).map_err(|e| format!("{e:?}"))?;
    let resources = embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let snapshot = SettingsSnapshot {
        revision: 7,
        schema_version: 2,
        ..SettingsSnapshot::default()
    };
    for (query, field) in [
        ("locale.display_language", Field::Key),
        ("DISPLAY LANGUAGE", Field::Name("en")),
        ("표시 언어", Field::Name("ko")),
        ("application", Field::Description("en")),
        ("앱에서", Field::Description("ko")),
    ] {
        let result = registry
            .search(&snapshot, &resources, query)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(result.revision, 7);
        let item = result
            .hits
            .iter()
            .find(|hit| hit.entry.descriptor.key == "locale.display_language")
            .ok_or("missing match")?;
        assert!(item.entry.uses_default);
        let matched = item
            .matches
            .iter()
            .find(|value| value.field == field)
            .ok_or("missing field")?;
        assert_eq!(&matched.normalized[matched.range.clone()], normalize(query));
    }
    assert!(
        registry
            .search(&snapshot, &resources, "no such value")
            .map_err(|e| format!("{e:?}"))?
            .hits
            .is_empty()
    );
    assert!(matches!(
        registry.search(&snapshot, &resources, ""),
        Err(Error::EmptyQuery)
    ));
    Ok(())
}

#[test]
fn missing_language_or_message_never_returns_partial_hits() -> Result<(), Box<dyn std::error::Error>>
{
    let descriptors =
        super::super::builtin_descriptors(&["en", "ko"]).map_err(|e| format!("{e:?}"))?;
    let registry = Registry::new(descriptors.clone()).map_err(|e| format!("{e:?}"))?;
    let all = embedded::resources().map_err(|e| format!("{e:?}"))?;
    let english: Vec<_> = all
        .into_iter()
        .filter(|resource| resource.language == "en")
        .collect();
    let resources = i18n::Registry::register(&english, &[], &[], embedded::parse_bundle)
        .map_err(|e| format!("{e:?}"))?;
    let snapshot = SettingsSnapshot::default();
    assert!(matches!(
        registry.search(&snapshot, &resources, "locale"),
        Err(Error::MissingLanguage)
    ));
    let mut broken = descriptors;
    broken[0].presentation.description_key = "missing.message".into();
    let registry = Registry::new(broken).map_err(|e| format!("{e:?}"))?;
    let full = embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        registry.search(&snapshot, &full, "locale"),
        Err(Error::MissingMessage)
    ));
    Ok(())
}

struct Store(SettingsSnapshot);
impl SettingsStore for Store {
    fn load(
        &mut self,
    ) -> Result<SettingsSnapshot, du_core_contracts::settings::SettingsStoreError> {
        Ok(self.0.clone())
    }
    fn commit(
        &mut self,
        _: u64,
        _: &du_core_contracts::settings::SettingsChanges,
        _: du_core_contracts::settings::SettingsCommit,
    ) -> Result<u64, du_core_contracts::settings::SettingsStoreError> {
        Err(du_core_contracts::settings::SettingsStoreError::Storage)
    }
}

#[test]
fn service_search_uses_same_revision_and_override_as_catalog()
-> Result<(), Box<dyn std::error::Error>> {
    let descriptors =
        super::super::builtin_descriptors(&["en", "ko"]).map_err(|e| format!("{e:?}"))?;
    let registry = Registry::new(descriptors).map_err(|e| format!("{e:?}"))?;
    let snapshot = SettingsSnapshot {
        revision: 9,
        schema_version: 2,
        values: BTreeMap::from([(
            "locale.display_language".to_owned(),
            serde_json::json!("ko"),
        )]),
    };
    let service = SettingsService::open(registry, Store(snapshot)).map_err(|e| format!("{e:?}"))?;
    let resources = embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let result = service
        .search(&resources, "locale.display_language")
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.revision, service.catalog().0);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].entry.value, &serde_json::json!("ko"));
    assert!(!result.hits[0].entry.uses_default);
    Ok(())
}

#[test]
fn korean_integrity_failure_degrades_search_without_hiding_it()
-> Result<(), Box<dyn std::error::Error>> {
    let descriptors =
        super::super::builtin_descriptors(&["en", "ko"]).map_err(|e| format!("{e:?}"))?;
    let registry = Registry::new(descriptors).map_err(|e| format!("{e:?}"))?;
    let mut rows: BTreeMap<_, _> = embedded::resources()
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .map(|row| (row.language, row))
        .collect();
    let en = rows.remove("en").ok_or("missing en")?;
    let ko = rows.remove("ko").ok_or("missing ko")?;
    let damaged = i18n::BundleResource {
        bytes: b"bad",
        ..ko
    };
    let resources = i18n::Registry::register(&[en, damaged], &[], &[], embedded::parse_bundle)
        .map_err(|e| format!("{e:?}"))?;
    let snapshot = SettingsSnapshot::default();
    let result = registry
        .search(&snapshot, &resources, "display language")
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.korean_integrity_degraded);
    assert!(
        result
            .hits
            .iter()
            .any(|hit| hit.entry.descriptor.key == "locale.display_language")
    );
    assert!(
        result
            .hits
            .iter()
            .flat_map(|hit| &hit.matches)
            .all(|m| !matches!(m.field, Field::Name("ko") | Field::Description("ko")))
    );
    Ok(())
}
