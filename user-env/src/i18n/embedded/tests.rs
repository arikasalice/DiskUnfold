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
fn setting_presentations_cover_eight_descriptors_in_both_bundles()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = register(&[]).map_err(|e| format!("{e:?}"))?;
    let descriptors: Vec<_> = crate::locale::descriptors(&["en", "ko"])
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .chain(crate::scope::descriptors())
        .chain(crate::space::descriptors())
        .collect();
    assert_eq!(descriptors.len(), 8);
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
