//! Cross-check of the published reason keys against the embedded reason metadata (FR5.HD1, NFR1).

use std::collections::BTreeSet;

use du_core_contracts::locale_bundle::MessageTemplate;

use super::{Registry, embedded};

/// Source of the shared error envelope; `code_for` is the published reason key list.
const ENVELOPES: &str = include_str!("../../../core-contracts/src/envelopes.rs");
const REASON_METADATA: &str = include_str!("../../i18n/reason-messages.json");

/// Reason keys owned by F13 that do not travel in an error envelope.
const LOCALE_KEYS_OUTSIDE_ENVELOPES: &[&str] = &[
    "locale.translation_fallback",
    "i18n.bundle_integrity_failed",
];

fn is_f13_key(key: &str) -> bool {
    key.starts_with("locale.") || key.starts_with("i18n.")
}

/// String literals in the `code_for` match, which is the published reason key list.
fn envelope_reason_keys() -> Result<BTreeSet<String>, String> {
    let start = ENVELOPES.find("fn code_for").ok_or("code_for not found")?;
    let end = ENVELOPES[start..]
        .find("fn new(")
        .map(|offset| start + offset)
        .ok_or("end of code_for not found")?;
    let mut keys = BTreeSet::new();
    for (index, part) in ENVELOPES[start..end].split('"').enumerate() {
        // Odd segments are inside quotes.
        if index % 2 == 1 && part.contains('.') {
            keys.insert(part.to_owned());
        }
    }
    if keys.len() < 30 {
        return Err(format!("too few reason keys parsed: {}", keys.len()));
    }
    Ok(keys)
}

fn metadata_keys() -> Result<BTreeSet<String>, String> {
    let rows: Vec<(String, serde_json::Value)> =
        serde_json::from_str(REASON_METADATA).map_err(|error| error.to_string())?;
    Ok(rows.into_iter().map(|(key, _)| key).collect())
}

fn text(registry: &Registry, language: &str, key: &str) -> Option<String> {
    let bundle = registry.bundle(language)?;
    bundle
        .messages
        .iter()
        .find_map(|(candidate, message)| (candidate == key).then(|| message.text.clone()))
}

#[test]
fn every_f13_reason_key_has_a_metadata_row_and_both_language_texts() -> Result<(), String> {
    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let metadata = metadata_keys()?;
    let mut f13_keys: BTreeSet<String> = envelope_reason_keys()?
        .into_iter()
        .filter(|key| is_f13_key(key))
        .collect();
    f13_keys.extend(
        LOCALE_KEYS_OUTSIDE_ENVELOPES
            .iter()
            .map(ToString::to_string),
    );
    // The envelope list must keep naming the keys this test knows about.
    for key in ["locale.unsupported", "locale.registration_failed"] {
        assert!(f13_keys.contains(key), "{key}");
    }
    for key in &f13_keys {
        assert!(metadata.contains(key), "reason metadata row missing: {key}");
        assert!(registry.resolve_reason("en", key).known_reason, "{key}");
        for language in ["en", "ko"] {
            let resolved = registry.resolve_reason(language, key);
            let action = resolved.action.ok_or_else(|| format!("{key}: no action"))?;
            for message in [resolved.cause, action] {
                assert_eq!(message.language, language, "{key}");
                assert!(message.marker.is_none(), "{key} {language}");
                let MessageTemplate::Text { template, .. } = message.template else {
                    return Err(format!("{key} {language}: not a text template"));
                };
                assert!(!template.trim().is_empty(), "{key} {language}");
            }
        }
    }
    // The metadata table holds no F13 row that the contract does not know.
    for key in metadata.iter().filter(|key| is_f13_key(key)) {
        assert!(f13_keys.contains(key), "unexpected F13 metadata row: {key}");
    }
    Ok(())
}

#[test]
fn reasons_owned_by_other_features_use_the_generic_message() -> Result<(), String> {
    let registry = embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    let metadata = metadata_keys()?;
    let others: Vec<String> = envelope_reason_keys()?
        .into_iter()
        .filter(|key| !is_f13_key(key) && !metadata.contains(key))
        .collect();
    assert!(!others.is_empty());
    for language in ["en", "ko"] {
        let generic = text(&registry, language, "ui.common.error_generic")
            .ok_or("generic message missing")?;
        for key in &others {
            let resolved = registry.resolve_reason(language, key);
            assert!(!resolved.known_reason, "{key}");
            assert!(resolved.action.is_none(), "{key}: guessed action");
            assert_eq!(resolved.reason_key, *key);
            let MessageTemplate::Text { template, .. } = resolved.cause.template else {
                return Err(format!("{key}: not a text template"));
            };
            assert_eq!(template, generic, "{key} {language}");
        }
    }
    Ok(())
}
