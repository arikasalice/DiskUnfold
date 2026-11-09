use super::*;

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
