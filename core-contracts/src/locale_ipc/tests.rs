use super::*;
use serde_json::{Value, json};

#[test]
fn profile_revision_requires_canonical_u64_decimal_string() {
    assert_eq!(
        serde_json::from_value::<LocaleProfileRevision>(json!("18446744073709551615"))
            .unwrap()
            .as_u64(),
        u64::MAX
    );
    for value in [
        json!(""),
        json!("00"),
        json!("+1"),
        json!("-1"),
        json!("18446744073709551616"),
        json!(1),
        Value::Null,
    ] {
        assert!(serde_json::from_value::<LocaleProfileRevision>(value).is_err());
    }
}

#[test]
fn get_profile_request_is_an_empty_object() {
    assert!(serde_json::from_value::<LocaleGetProfileRequest>(json!({})).is_ok());
    for value in [json!({"future": true}), json!([]), Value::Null, json!(1)] {
        assert!(serde_json::from_value::<LocaleGetProfileRequest>(value).is_err());
    }
}

#[test]
fn set_profile_changes_allow_omission_but_reject_empty_null_and_unknown_fields() {
    let request: LocaleProfileSetRequest = serde_json::from_value(json!({
        "expectedProfileRevision": "0",
        "changes": {"dateFormat": "iso8601"}
    }))
    .unwrap();
    assert_eq!(request.changes.date_format.as_deref(), Some("iso8601"));

    for value in [
        json!({"expectedProfileRevision":"0","changes":{}}),
        json!({"expectedProfileRevision":"0","changes":{"dateFormat":null}}),
        json!({"expectedProfileRevision":"0","changes":{"future":true}}),
        json!({"expectedProfileRevision":"0","changes":{"appScale":null}}),
        json!({"expectedProfileRevision":null,"changes":{"motion":"reduce"}}),
        json!({"expectedProfileRevision":"0","changes":{"motion":"reduce"},"future":true}),
    ] {
        assert!(serde_json::from_value::<LocaleProfileSetRequest>(value).is_err());
    }
    assert!(
        serde_json::from_value::<LocaleProfileSetRequest>(json!([
            "0",
            {"displayLanguage":"ko"}
        ]))
        .is_err()
    );
    assert!(
        serde_json::from_value::<LocaleProfileSetRequest>(json!({
            "expectedProfileRevision": "0",
            "changes": ["ko"]
        }))
        .is_err()
    );
    let raw_value: LocaleProfileSetRequest = serde_json::from_value(json!({
        "expectedProfileRevision": "0",
        "changes": {"dateFormat": "unsupported"}
    }))
    .unwrap();
    assert_eq!(
        raw_value.changes.date_format.as_deref(),
        Some("unsupported")
    );
}

#[test]
fn app_scale_accepts_only_published_percentages() {
    for percent in LocaleAppScale::ALLOWED_PERCENTAGES {
        assert_eq!(
            serde_json::from_value::<LocaleAppScale>(json!(percent))
                .unwrap()
                .percent(),
            percent
        );
    }
    assert_eq!(
        serde_json::to_value(LocaleAppScale::new(100).unwrap()).unwrap(),
        json!(100)
    );
    for percent in [0, 99, 101, 120, 250] {
        assert!(serde_json::from_value::<LocaleAppScale>(json!(percent)).is_err());
    }
}

#[test]
fn locale_enums_use_the_published_snake_case_spellings() {
    assert_eq!(
        serde_json::to_value(LocaleLanguageSource::Os).unwrap(),
        json!("os")
    );
    assert_eq!(
        serde_json::to_value(LocaleDateFormat::LangDefault).unwrap(),
        json!("lang_default")
    );
    assert_eq!(
        serde_json::to_value(LocaleNumberFormat::NoGrouping).unwrap(),
        json!("no_grouping")
    );
    assert_eq!(
        serde_json::to_value(LocalTimeZoneStatus::OffsetOnly).unwrap(),
        json!("offset_only")
    );
    assert_eq!(
        serde_json::to_value(LocaleMotion::FollowOs).unwrap(),
        json!("follow_os")
    );
}

#[test]
fn locale_changed_fields_must_be_unique_and_in_declared_order() {
    let event: LocaleChangedBody = serde_json::from_value(json!({
        "profileRevision": "18446744073709551615",
        "changedFields": ["display_language", "app_scale"],
        "displayLanguage": "ko",
        "future": true
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        json!({
            "profileRevision": "18446744073709551615",
            "changedFields": ["display_language", "app_scale"],
            "displayLanguage": "ko"
        })
    );

    for value in [
        json!({"profileRevision":"1","changedFields":[],"displayLanguage":"ko"}),
        json!({"profileRevision":"1","changedFields":["app_scale","motion"],"displayLanguage":"ko"}),
        json!({"profileRevision":"1","changedFields":["motion","motion"],"displayLanguage":"ko"}),
    ] {
        assert!(serde_json::from_value::<LocaleChangedBody>(value).is_err());
    }
}
