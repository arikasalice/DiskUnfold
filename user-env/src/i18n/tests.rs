use super::*;
use du_core_contracts::locale_bundle::{
    LocaleMissingEntry, LocaleMissingMarker, LocaleReportMissingRequest,
    LocaleReportMissingResponse,
};

fn parse(bytes: &[u8]) -> Result<Bundle, RegistrationError> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| RegistrationError)?;
    let language = value["language"].as_str().ok_or(RegistrationError)?;
    let mut messages = Vec::new();
    for entry in value["messages"].as_array().ok_or(RegistrationError)? {
        let key = entry[0].as_str().ok_or(RegistrationError)?;
        let kind = entry[1].as_str().ok_or(RegistrationError)?;
        messages.push((
            key.to_owned(),
            Message {
                text: key.to_owned(),
                parameters: BTreeMap::from([("count".to_owned(), kind.to_owned())]),
                plural: None,
            },
        ));
    }
    Ok(Bundle {
        language: language.to_owned(),
        version: "1".to_owned(),
        messages,
        format_table: serde_json::json!({}),
        plural_rule: "default".to_owned(),
        fallback_language: "en".to_owned(),
    })
}

const EN: &[u8] = br#"{"language":"en","messages":[["hello","count"]]}"#;
const KO: &[u8] = br#"{"language":"ko","messages":[["hello","count"]]}"#;

fn resource<'a>(language: &'a str, bytes: &'a [u8]) -> BundleResource<'a> {
    BundleResource {
        language,
        version: "1",
        bytes,
        expected_hash: *blake3::hash(bytes).as_bytes(),
    }
}

fn register(
    resources: &[BundleResource<'_>],
    keys: &[&str],
    metadata: &[&str],
) -> Result<Registry, RegistrationError> {
    Registry::register(
        resources,
        &keys.iter().map(ToString::to_string).collect::<Vec<_>>(),
        &metadata.iter().map(ToString::to_string).collect::<Vec<_>>(),
        parse,
    )
}

#[test]
fn registers_and_checks_parameters() -> Result<(), RegistrationError> {
    let registry = register(
        &[resource("en", EN), resource("ko", KO)],
        &["hello"],
        &["reason"],
    )?;
    assert!(registry.bundle("en").is_some());
    assert!(registry.bundle("ko").is_some());
    assert!(registry.integrity_warnings().is_empty());
    let mut declarations = BTreeMap::from([(
        "hello".to_owned(),
        BTreeMap::from([("count".to_owned(), "count".to_owned())]),
    )]);
    assert!(parameter_mismatches(&registry, &declarations).is_empty());
    declarations.insert(
        "hello".to_owned(),
        BTreeMap::from([("count".to_owned(), "number".to_owned())]),
    );
    assert_eq!(parameter_mismatches(&registry, &declarations).len(), 2);
    Ok(())
}

#[test]
fn embedded_cli_settings_unavailable_message_is_registered_in_both_languages()
-> Result<(), RegistrationError> {
    let registry = super::embedded::register(&[])?;
    assert!(
        super::embedded::declared_keys()?
            .iter()
            .any(|key| key == "cli.settings.unavailable")
    );

    for (language, expected) in [
        (
            "en",
            "Saved settings are unavailable. Using a fallback language.",
        ),
        ("ko", "저장된 설정을 사용할 수 없어 대체 언어를 사용합니다."),
    ] {
        let text = registry
            .bundle(language)
            .and_then(|bundle| {
                bundle.messages.iter().find_map(|(key, message)| {
                    (key == "cli.settings.unavailable").then_some(message.text.as_str())
                })
            })
            .ok_or(RegistrationError)?;
        assert_eq!(text, expected);
    }
    Ok(())
}

#[test]
fn rejects_missing_duplicate_and_mismatched_registration() {
    assert!(register(&[resource("en", EN)], &["hello", "missing"], &[]).is_err());
    assert!(register(&[resource("en", EN), resource("en", EN)], &[], &[]).is_err());
    assert!(register(&[resource("en", EN)], &[], &["reason", "reason"]).is_err());
    assert!(register(&[resource("en", EN)], &["hello", "hello"], &[]).is_err());
    let duplicate = br#"{"language":"en","messages":[["hello","count"],["hello","count"]]}"#;
    assert!(register(&[resource("en", duplicate)], &[], &[]).is_err());
    assert!(register(&[resource("en", KO)], &[], &[]).is_err());
    assert!(register(&[], &[], &[]).is_err());
    assert_eq!(RegistrationError.reason_key(), "locale.registration_failed");
}

#[test]
fn only_korean_integrity_failure_allows_english_to_continue() -> Result<(), RegistrationError> {
    let mut ko = resource("ko", KO);
    ko.expected_hash = [0; 32];
    let registry = register(&[resource("en", EN), ko], &["hello"], &[])?;
    assert!(registry.bundle("en").is_some());
    assert!(registry.bundle("ko").is_none());
    assert_eq!(
        registry.integrity_warnings(),
        ["i18n.bundle_integrity_failed"]
    );
    let mut en = resource("en", EN);
    en.expected_hash = [0; 32];
    assert!(register(&[en], &[], &[]).is_err());
    Ok(())
}
#[test]
fn published_bundle_revision_vectors() {
    let resource = *blake3::hash(b"resource").as_bytes();
    let metadata = blake3::hash(b"meta");
    assert_eq!(
        metadata.to_hex().as_str(),
        "37b2bcaae91385dae953ccf1688ea09d6655c29b2e2594d6656b609a0163b37d"
    );
    assert_eq!(
        super::bundle_revision("en", &resource, metadata.as_bytes()).as_deref(),
        Some("90cb92f8873b47b4b98d516402c508458403067b2c234d8b27a66183a2ef9898")
    );
    assert_eq!(
        super::bundle_revision("ko", &resource, metadata.as_bytes()).as_deref(),
        Some("e9c33453b4b63650c7c9e6df6dd2c760b56f43da99b2a3941ae335147c82a80b")
    );
    let changed = blake3::hash(b"meta2");
    assert_ne!(
        super::bundle_revision("en", &resource, metadata.as_bytes()),
        super::bundle_revision("en", &resource, changed.as_bytes())
    );
}

#[test]
fn embedded_inventory_is_derived_from_verified_bytes() -> Result<(), super::RegistrationError> {
    let registry = super::embedded::register(&[])?;
    for resource in super::embedded::resources()? {
        assert_eq!(
            registry.resource_digest(resource.language),
            Some(*blake3::hash(resource.bytes).as_bytes())
        );
        let key = registry
            .self_name_key(resource.language)
            .ok_or(super::RegistrationError)?;
        assert!(
            registry
                .bundle(resource.language)
                .ok_or(super::RegistrationError)?
                .messages
                .iter()
                .any(|(candidate, _)| candidate == key)
        );
        assert!(
            registry
                .revisions(resource.language, b"actual metadata bytes")
                .is_some()
        );
    }
    assert!(registry.revisions("unknown", b"meta").is_none());
    Ok(())
}

#[test]
fn korean_integrity_failure_excludes_evidence_and_revisions() -> Result<(), super::RegistrationError>
{
    let resources = super::embedded::resources()?;
    let english = resources
        .iter()
        .find(|r| r.language == "en")
        .ok_or(super::RegistrationError)?;
    let korean = resources
        .iter()
        .find(|r| r.language == "ko")
        .ok_or(super::RegistrationError)?;
    let damaged = super::BundleResource {
        language: korean.language,
        version: korean.version,
        bytes: korean.bytes,
        expected_hash: [0; 32],
    };
    let english = super::BundleResource {
        language: english.language,
        version: english.version,
        bytes: english.bytes,
        expected_hash: english.expected_hash,
    };
    let registry =
        super::Registry::register(&[english, damaged], &[], &[], super::embedded::parse_bundle)?;
    assert!(registry.self_name_key("ko").is_none());
    assert!(registry.resource_digest("ko").is_none());
    assert!(registry.revisions("ko", b"meta").is_none());
    assert!(registry.self_name_key("en").is_some());
    Ok(())
}

#[test]
fn missing_report_filters_keys_deduplicates_and_keeps_only_the_newest_thousand()
-> Result<(), RegistrationError> {
    let registry = register(&[resource("en", EN), resource("ko", KO)], &[], &[])?;
    let fallback = LocaleReportMissingRequest {
        language: "ko".into(),
        entries: vec![
            LocaleMissingEntry {
                key: "ui.fixture.missing".into(),
                marker: LocaleMissingMarker::Fallback,
            },
            LocaleMissingEntry {
                key: "ui.fixture.missing".into(),
                marker: LocaleMissingMarker::Key,
            },
            LocaleMissingEntry {
                key: "../private/path".into(),
                marker: LocaleMissingMarker::Key,
            },
            LocaleMissingEntry {
                key: "x".repeat(257),
                marker: LocaleMissingMarker::Key,
            },
        ],
    };
    assert_eq!(
        registry.report_missing(&fallback),
        LocaleReportMissingResponse {}
    );
    let retained = registry.missing_messages();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].key, "ui.fixture.missing");
    assert_eq!(retained[0].marker, LocaleMissingMarker::Key);

    registry.report_missing(&LocaleReportMissingRequest {
        language: "fr".into(),
        entries: vec![LocaleMissingEntry {
            key: "ui.fixture.unknown_language".into(),
            marker: LocaleMissingMarker::Fallback,
        }],
    });
    assert_eq!(registry.missing_messages().len(), 1);

    let entries = (1..=1_001)
        .map(|index| LocaleMissingEntry {
            key: format!("ui.fixture.item_{index:04}"),
            marker: LocaleMissingMarker::Fallback,
        })
        .collect();
    registry.report_missing(&LocaleReportMissingRequest {
        language: "en".into(),
        entries,
    });
    let retained = registry.missing_messages();
    assert_eq!(retained.len(), MISSING_MESSAGE_CAPACITY);
    assert_eq!(retained[0].key, "ui.fixture.item_0002");
    assert_eq!(retained[999].key, "ui.fixture.item_1001");
    Ok(())
}

#[test]
fn dropped_missing_count_tracks_only_evictions() -> Result<(), RegistrationError> {
    let registry = register(&[resource("en", EN), resource("ko", KO)], &[], &[])?;
    let report = |language: &str, keys: Vec<String>| {
        registry.report_missing(&LocaleReportMissingRequest {
            language: language.into(),
            entries: keys
                .into_iter()
                .map(|key| LocaleMissingEntry {
                    key,
                    marker: LocaleMissingMarker::Fallback,
                })
                .collect(),
        });
    };
    let key = |index: usize| format!("ui.fixture.item_{index:04}");
    report("en", (1..=1_000).map(key).collect());
    assert_eq!(registry.dropped_missing_messages(), 0);
    report("en", vec![key(1), key(500)]);
    report("fr", vec!["ui.fixture.other".into()]);
    report("en", vec!["../bad".into(), "Bad".into()]);
    assert_eq!(registry.dropped_missing_messages(), 0);
    report("en", (1_001..=1_003).map(key).collect());
    assert_eq!(registry.dropped_missing_messages(), 3);
    let retained = registry.missing_messages();
    assert_eq!(retained.len(), MISSING_MESSAGE_CAPACITY);
    assert_eq!(retained[0].key, key(4));
    // The same key in another registered language is a distinct entry.
    report("ko", vec![key(1_003)]);
    assert_eq!(registry.dropped_missing_messages(), 4);
    Ok(())
}
