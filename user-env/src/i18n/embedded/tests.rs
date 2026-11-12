use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::i18n::parameter_mismatches;

fn built_in_pair() -> Result<[BundleResource<'static>; 2], RegistrationError> {
    let mut rows: BTreeMap<_, _> = resources()?
        .into_iter()
        .map(|row| (row.language, row))
        .collect();
    Ok([
        rows.remove("en").ok_or(RegistrationError)?,
        rows.remove("ko").ok_or(RegistrationError)?,
    ])
}

#[test]
fn built_in_resources_register_with_exact_declared_keys_and_parameters()
-> Result<(), RegistrationError> {
    let registry = register(&[])?;
    let declared: BTreeSet<_> = DECLARED_KEYS.iter().copied().collect();
    assert_eq!(declared.len(), DECLARED_KEYS.len());
    let declarations: BTreeMap<_, _> = DECLARED_KEYS
        .iter()
        .map(|key| ((*key).to_owned(), BTreeMap::new()))
        .collect();
    for language in ["en", "ko"] {
        let bundle = registry.bundle(language).ok_or(RegistrationError)?;
        let keys: BTreeSet<_> = bundle
            .messages
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys, declared);
        assert_eq!(keys.len(), bundle.messages.len());
    }
    assert!(parameter_mismatches(&registry, &declarations).is_empty());
    assert!(registry.integrity_warnings().is_empty());
    let english = registry.bundle("en").ok_or(RegistrationError)?;
    assert!(
        english
            .messages
            .iter()
            .any(|(key, message)| key == "cli.help.usage"
                && message.text == "Usage: diskunfold [--lang LANGUAGE] [--help | --version]")
    );
    Ok(())
}

#[test]
fn built_in_digests_detect_tampering_before_parsing() -> Result<(), RegistrationError> {
    let [en, ko] = built_in_pair()?;
    assert_eq!(en.bytes, include_bytes!("../../../i18n/en.json"));
    assert_eq!(ko.bytes, include_bytes!("../../../i18n/ko.json"));
    assert_eq!(en.expected_hash, *blake3::hash(en.bytes).as_bytes());
    assert_eq!(ko.expected_hash, *blake3::hash(ko.bytes).as_bytes());
    let declarations: Vec<_> = DECLARED_KEYS.iter().map(|key| (*key).to_owned()).collect();
    let damaged_ko = BundleResource {
        bytes: b"tampered",
        ..ko
    };
    let registry = Registry::register(&[en, damaged_ko], &declarations, &[], parse_bundle)?;
    assert!(registry.bundle("en").is_some());
    assert!(registry.bundle("ko").is_none());
    assert_eq!(
        registry.integrity_warnings(),
        ["i18n.bundle_integrity_failed"]
    );
    let [en, ko] = built_in_pair()?;
    let damaged_en = BundleResource {
        bytes: b"tampered",
        ..en
    };
    assert!(Registry::register(&[damaged_en, ko], &declarations, &[], parse_bundle).is_err());
    Ok(())
}

#[test]
fn parser_rejects_malformed_rows_and_duplicate_parameter_names() {
    for bytes in [
        &b"not json"[..],
        &br#"["en","1",{},"en","en",[["key","text",[["n","count"],["n","size"]]]]]"#[..],
        &br#"["en","1",{},"en","en",[["key","text"]]]"#[..],
    ] {
        assert!(parse_bundle(bytes).is_err());
    }
}

#[test]
fn registration_rejects_duplicate_keys_and_wrong_metadata() -> Result<(), RegistrationError> {
    let bytes = br#"["en","1",{},"en","en",[["key","a",[]],["key","b",[]]]]"#;
    let resource = BundleResource {
        language: "en",
        version: "1",
        bytes,
        expected_hash: *blake3::hash(bytes).as_bytes(),
    };
    assert!(Registry::register(&[resource], &[], &[], parse_bundle).is_err());
    let [en, _] = built_in_pair()?;
    let wrong_version = BundleResource { version: "2", ..en };
    assert!(Registry::register(&[wrong_version], &[], &[], parse_bundle).is_err());
    Ok(())
}

#[test]
fn setting_presentations_cover_descriptors_in_both_bundles()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = register(&[]).map_err(|e| format!("{e:?}"))?;
    let descriptors =
        crate::settings::builtin_descriptors(&["en", "ko"]).map_err(|e| format!("{e:?}"))?;
    assert_eq!(descriptors.len(), 11);
    validate_setting_presentation(&registry, descriptors.clone()).map_err(|e| format!("{e:?}"))?;
    for language in ["en", "ko"] {
        let bundle = registry.bundle(language).ok_or("missing bundle")?;
        for descriptor in &descriptors {
            for key in [
                &descriptor.presentation.name_key,
                &descriptor.presentation.description_key,
            ] {
                let message = bundle
                    .messages
                    .iter()
                    .find(|(candidate, _)| candidate == key)
                    .ok_or("missing message")?;
                assert!(!message.1.text.trim().is_empty());
                assert!(message.1.parameters.is_empty());
            }
        }
    }
    Ok(())
}

#[test]
fn setting_presentation_rejects_duplicate_empty_and_missing_keys()
-> Result<(), Box<dyn std::error::Error>> {
    let mut registry = register(&[]).map_err(|e| format!("{e:?}"))?;
    let descriptors = crate::scope::descriptors();
    let one = descriptors.first().ok_or("missing descriptor")?.clone();
    assert!(validate_setting_presentation(&registry, [one.clone(), one.clone()]).is_err());
    let mut same = one.clone();
    same.presentation.description_key = same.presentation.name_key.clone();
    assert!(validate_setting_presentation(&registry, [same]).is_err());
    let mut empty = one.clone();
    empty.presentation.name_key.clear();
    assert!(validate_setting_presentation(&registry, [empty]).is_err());
    let mut missing = one.clone();
    missing.presentation.name_key = "settings.missing.name".into();
    assert!(validate_setting_presentation(&registry, [missing]).is_err());
    registry
        .bundles
        .get_mut("ko")
        .ok_or("missing ko")?
        .messages
        .retain(|(key, _)| key != &one.presentation.description_key);
    assert!(validate_setting_presentation(&registry, [one]).is_err());
    Ok(())
}

#[test]
fn registered_catalog_carries_only_published_group_and_level_pairs()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::settings::{
        SettingGroup as G, SettingLevel as L, SettingPlacement, SettingsSnapshot,
    };
    let descriptors = crate::locale::descriptors(&["en", "ko"])
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .chain(crate::scope::descriptors())
        .chain(crate::space::descriptors());
    let registry = crate::Registry::new(descriptors).map_err(|e| format!("{e:?}"))?;
    let snapshot = SettingsSnapshot {
        revision: 17,
        schema_version: 2,
        values: BTreeMap::from([("scope.remember_recent".into(), serde_json::json!(false))]),
    };
    let before = snapshot.clone();
    let rows = registry.catalog(&snapshot);
    let placements: BTreeMap<_, _> = rows
        .iter()
        .map(|row| {
            (
                row.descriptor.key.as_str(),
                row.descriptor.presentation.placement,
            )
        })
        .collect();
    let pair = |group, level| Some(SettingPlacement { group, level });
    assert_eq!(placements.len(), 11);
    for key in [
        "display.size_unit_system",
        "display.size_unit",
        "space.default_basis",
    ] {
        assert_eq!(placements[key], None);
    }
    assert_eq!(
        placements["locale.display_language"],
        pair(G::GeneralDisplay, L::Basic)
    );
    for key in [
        "locale.date_format",
        "locale.number_format",
        "locale.time_zone_policy",
    ] {
        assert_eq!(placements[key], pair(G::GeneralDisplay, L::Detailed));
    }
    for key in ["a11y.motion", "a11y.app_scale"] {
        assert_eq!(placements[key], pair(G::Accessibility, L::Basic));
    }
    assert_eq!(
        placements["scope.remember_recent"],
        pair(G::HistoryPrivacy, L::Basic)
    );
    assert_eq!(placements["space.shared_table_max_entries"], None);
    let recent = rows
        .iter()
        .find(|row| row.descriptor.key == "scope.remember_recent")
        .ok_or("missing scope")?;
    assert_eq!(recent.value, &serde_json::json!(false));
    assert!(!recent.uses_default);
    assert_eq!(snapshot, before);
    Ok(())
}
