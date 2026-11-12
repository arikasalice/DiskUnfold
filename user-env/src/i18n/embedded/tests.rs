use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::i18n::declared::DECLARED_PARAMETERS;
use crate::i18n::parameter_mismatches;
use du_core_contracts::settings::SETTINGS_SCHEMA_VERSION;
use serde_json::{Value, json};

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

fn altered_bundle(
    language: &str,
    alter: impl FnOnce(&mut Value),
) -> Result<Vec<u8>, RegistrationError> {
    let resource = resources()?
        .into_iter()
        .find(|resource| resource.language == language)
        .ok_or(RegistrationError)?;
    let mut bundle: Value =
        serde_json::from_slice(resource.bytes).map_err(|_| RegistrationError)?;
    alter(&mut bundle);
    serde_json::to_vec(&bundle).map_err(|_| RegistrationError)
}

fn register_with_overrides<'a>(
    overrides: &[(&str, &'a [u8])],
    additional: Option<BundleResource<'a>>,
) -> Result<Registry, RegistrationError> {
    let mut resources = resources()?
        .into_iter()
        .map(|resource| {
            if let Some((_, bytes)) = overrides
                .iter()
                .find(|(language, _)| *language == resource.language)
            {
                BundleResource {
                    bytes,
                    expected_hash: *blake3::hash(bytes).as_bytes(),
                    ..resource
                }
            } else {
                resource
            }
        })
        .collect::<Vec<_>>();
    if let Some(additional) = additional {
        resources.push(additional);
    }
    register_resources(&resources, &[])
}

#[test]
fn built_in_resources_register_with_exact_declared_keys_and_parameters()
-> Result<(), RegistrationError> {
    let registry = register(&[])?;
    let declared: BTreeSet<_> = DECLARED_KEYS.iter().copied().collect();
    assert_eq!(declared.len(), DECLARED_KEYS.len());
    let declarations: BTreeMap<_, _> = DECLARED_KEYS
        .iter()
        .map(|key| {
            let parameters = DECLARED_PARAMETERS
                .iter()
                .find_map(|(message_key, params)| (message_key == key).then_some(*params))
                .unwrap_or_default();
            (
                (*key).to_owned(),
                parameters
                    .iter()
                    .map(|(name, kind)| (name.to_string(), kind.to_string()))
                    .collect(),
            )
        })
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
    let korean = registry.bundle("ko").ok_or(RegistrationError)?;
    assert_eq!(english.fallback_language, "");
    assert_eq!(korean.fallback_language, "en");
    for bundle in [english, korean] {
        assert_eq!(bundle.format_table["decimalSeparator"].as_str(), Some("."));
        assert_eq!(bundle.format_table["groupingSeparator"].as_str(), Some(","));
        assert_eq!(bundle.format_table["groupingSize"].as_u64(), Some(3));
        assert_eq!(
            bundle.format_table["datePatterns"]
                .as_object()
                .map(|values| values.len()),
            Some(2)
        );
        assert_eq!(
            bundle.format_table["unitSymbols"]
                .as_object()
                .map(|values| values.len()),
            Some(11)
        );
    }
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
fn startup_rejects_malformed_format_tables() -> Result<(), RegistrationError> {
    for alter in [
        (|bundle: &mut Value| bundle[2]["groupingSize"] = json!(0)) as fn(&mut Value),
        |bundle: &mut Value| {
            if let Some(format_table) = bundle[2].as_object_mut() {
                format_table.remove("groupingSeparator");
            }
        },
        |bundle: &mut Value| {
            bundle[2]["datePatterns"]["iso8601"] = json!("yyyy-MM-dd'T'HH:qq:ss");
        },
        |bundle: &mut Value| {
            bundle[2]["unitSymbols"]["kB"] = json!("KB");
        },
    ] {
        let english = altered_bundle("en", alter)?;
        let result = register_with_overrides(&[("en", &english)], None);
        assert_eq!(
            result.err().map(|error| error.reason_key()),
            Some("locale.registration_failed")
        );
    }
    Ok(())
}

#[test]
fn startup_requires_english_omission_and_registered_fallbacks() -> Result<(), RegistrationError> {
    let english = altered_bundle("en", |bundle| bundle[4] = json!("en"))?;
    assert!(register_with_overrides(&[("en", &english)], None).is_err());

    for fallback in ["ko", "fr", ""] {
        let korean = altered_bundle("ko", |bundle| bundle[4] = json!(fallback))?;
        assert!(register_with_overrides(&[("ko", &korean)], None).is_err());
    }
    Ok(())
}

#[test]
fn startup_rejects_cyclic_fallbacks_and_accepts_one_added_language() -> Result<(), RegistrationError>
{
    let korean = altered_bundle("ko", |bundle| bundle[4] = json!("fr"))?;
    let additional_language = altered_bundle("en", |bundle| {
        bundle[0] = json!("fr");
        bundle[4] = json!("ko");
        if let Some(messages) = bundle[5].as_array_mut() {
            messages.push(json!(["locale.language.fr", "Test language", []]));
        }
    })?;
    let additional = BundleResource {
        language: "fr",
        version: "1",
        bytes: &additional_language,
        expected_hash: *blake3::hash(&additional_language).as_bytes(),
    };
    assert!(register_with_overrides(&[("ko", &korean)], Some(additional)).is_err());

    let additional_language = altered_bundle("en", |bundle| {
        bundle[0] = json!("fr");
        bundle[4] = json!("en");
        if let Some(messages) = bundle[5].as_array_mut() {
            messages.push(json!(["locale.language.fr", "Test language", []]));
        }
    })?;
    let additional = BundleResource {
        language: "fr",
        version: "1",
        bytes: &additional_language,
        expected_hash: *blake3::hash(&additional_language).as_bytes(),
    };
    let registry = register_with_overrides(&[], Some(additional))?;
    assert_eq!(registry.self_name_key("fr"), Some("locale.language.fr"));
    Ok(())
}

#[test]
fn output_interrupted_message_registers_in_both_built_in_languages() -> Result<(), RegistrationError>
{
    let registry = register(&[])?;
    for (language, expected_text) in [
        ("en", "Output was interrupted."),
        ("ko", "출력이 중단됐습니다."),
    ] {
        let bundle = registry.bundle(language).ok_or(RegistrationError)?;
        let message = bundle
            .messages
            .iter()
            .find(|(key, _)| key == "cli.output.interrupted")
            .ok_or(RegistrationError)?;
        assert_eq!(message.1.text, expected_text);
    }
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
fn parser_rejects_malformed_rows_and_duplicate_parameter_names() -> Result<(), RegistrationError> {
    for bytes in [&b"not json"[..], &b"[]"[..]] {
        assert!(parse_bundle(bytes).is_err());
    }
    let duplicate_parameters = altered_bundle("en", |bundle| {
        bundle[5][0][2] = json!([["n", "count"], ["n", "size"]]);
    })?;
    assert!(parse_bundle(&duplicate_parameters).is_err());
    Ok(())
}

#[test]
fn registration_rejects_duplicate_keys_and_wrong_metadata() -> Result<(), RegistrationError> {
    let bytes = altered_bundle("en", |bundle| {
        let key = bundle[5][0][0].clone();
        bundle[5][1][0] = key;
    })?;
    let resource = BundleResource {
        language: "en",
        version: "1",
        bytes: &bytes,
        expected_hash: *blake3::hash(&bytes).as_bytes(),
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
    assert_eq!(descriptors.len(), 12);
    validate_setting_presentation(&registry, descriptors.clone()).map_err(|e| format!("{e:?}"))?;
    let recent_limit = descriptors
        .iter()
        .find(|descriptor| descriptor.key == "scope.recent_limit")
        .ok_or("missing recent-scope limit")?;
    assert_eq!(
        recent_limit.presentation.name_key,
        "settings.scope.recent_limit.name"
    );
    assert_eq!(
        recent_limit.presentation.description_key,
        "settings.scope.recent_limit.description"
    );
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
        schema_version: SETTINGS_SCHEMA_VERSION,
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
    assert_eq!(placements.len(), 12);
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
    assert_eq!(
        placements["scope.recent_limit"],
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
