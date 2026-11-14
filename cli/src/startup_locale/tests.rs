//! Language selection policy tests with injected stored and OS readers.

use super::*;

#[test]
fn stored_language_short_circuits_os_lookup() -> Result<(), Box<dyn std::error::Error>> {
    let registry = i18n::embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let mut os_calls = 0;
    let (decision, unavailable) = select_with(
        &registry,
        None,
        || Ok(Some("en".into())),
        || {
            os_calls += 1;
            None
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(os_calls, 0);
    assert_eq!(decision.language, "en");
    assert_eq!(decision.source, locale::Source::Stored);
    assert!(!unavailable);
    Ok(())
}

#[test]
fn skipped_settings_uses_os_language_without_reading_stored_port()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = i18n::embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let mut stored_calls = 0;
    let (decision, unavailable) = select_with_policy(
        &registry,
        None,
        true,
        || {
            stored_calls += 1;
            Err(())
        },
        || Some(vec!["ko-KR".into()]),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stored_calls, 0);
    assert_eq!(decision.language, "ko");
    assert_eq!(decision.source, locale::Source::Os);
    assert!(!unavailable);
    Ok(())
}

#[test]
fn skipped_settings_uses_english_os_language() -> Result<(), Box<dyn std::error::Error>> {
    let registry = i18n::embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let mut stored_calls = 0;
    let (decision, unavailable) = select_with_policy(
        &registry,
        None,
        true,
        || {
            stored_calls += 1;
            Err(())
        },
        || Some(vec!["en-US".into()]),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stored_calls, 0);
    assert_eq!(decision.language, "en");
    assert_eq!(decision.source, locale::Source::Os);
    assert!(!unavailable);
    Ok(())
}

#[test]
fn skipped_settings_falls_back_when_os_language_is_unregistered()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = i18n::embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let mut stored_calls = 0;
    let (decision, unavailable) = select_with_policy(
        &registry,
        None,
        true,
        || {
            stored_calls += 1;
            Err(())
        },
        || Some(vec!["fr-FR".into()]),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stored_calls, 0);
    assert_eq!(decision.language, "en");
    assert_eq!(decision.source, locale::Source::Baseline);
    assert!(!unavailable);
    Ok(())
}

#[test]
fn skipped_settings_falls_back_to_english_without_os_language()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = i18n::embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let (decision, unavailable) = select_with_policy(&registry, None, true, || Err(()), || None)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(decision.language, "en");
    assert_eq!(decision.source, locale::Source::Baseline);
    assert!(!unavailable);
    Ok(())
}

#[test]
fn explicit_language_skips_both_settings_and_os_lookups() -> Result<(), Box<dyn std::error::Error>>
{
    let registry = i18n::embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let mut stored_calls = 0;
    let mut os_calls = 0;
    let (decision, unavailable) = select_with_policy(
        &registry,
        Some("en"),
        true,
        || {
            stored_calls += 1;
            Err(())
        },
        || {
            os_calls += 1;
            Some(vec!["ko-KR".into()])
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stored_calls, 0);
    assert_eq!(os_calls, 0);
    assert_eq!(decision.language, "en");
    assert_eq!(decision.source, locale::Source::Argument);
    assert!(!unavailable);
    Ok(())
}

#[test]
fn invalid_stored_language_uses_os_once() -> Result<(), Box<dyn std::error::Error>> {
    let registry = i18n::embedded::register(&[]).map_err(|e| format!("{e:?}"))?;
    let mut calls = 0;
    let (decision, _) = select_with(
        &registry,
        None,
        || Ok(Some("fr".into())),
        || {
            calls += 1;
            Some(vec!["ko-KR".into()])
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(calls, 1);
    assert_eq!(decision.language, "ko");
    assert!(decision.stored_unavailable);
    Ok(())
}

#[test]
fn korean_integrity_failure_skips_os_and_preserves_warning()
-> Result<(), Box<dyn std::error::Error>> {
    let mut resources = i18n::embedded::resources().map_err(|e| format!("{e:?}"))?;
    for resource in &mut resources {
        if resource.language == "ko" {
            resource.expected_hash = [0; 32];
        }
    }
    let registry = i18n::Registry::register(&resources, &[], &[], i18n::embedded::parse_bundle)
        .map_err(|e| format!("{e:?}"))?;
    let mut os_calls = 0;
    let (decision, _) = select_with(
        &registry,
        None,
        || Ok(Some("ko".into())),
        || {
            os_calls += 1;
            None
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(os_calls, 0);
    assert_eq!(decision.language, "en");
    assert_eq!(decision.source, locale::Source::Baseline);
    assert_eq!(
        registry.integrity_warnings(),
        ["i18n.bundle_integrity_failed"]
    );
    Ok(())
}

#[test]
fn skipped_settings_keeps_korean_integrity_warning_and_english_fallback()
-> Result<(), Box<dyn std::error::Error>> {
    let mut resources = i18n::embedded::resources().map_err(|e| format!("{e:?}"))?;
    for resource in &mut resources {
        if resource.language == "ko" {
            resource.expected_hash = [0; 32];
        }
    }
    let registry = i18n::Registry::register(&resources, &[], &[], i18n::embedded::parse_bundle)
        .map_err(|e| format!("{e:?}"))?;
    let mut stored_calls = 0;
    let mut os_calls = 0;
    let (decision, unavailable) = select_with_policy(
        &registry,
        None,
        true,
        || {
            stored_calls += 1;
            Err(())
        },
        || {
            os_calls += 1;
            Some(vec!["ko-KR".into()])
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stored_calls, 0);
    assert_eq!(os_calls, 0);
    assert_eq!(decision.language, "en");
    assert_eq!(decision.source, locale::Source::Baseline);
    assert!(!unavailable);
    assert_eq!(
        registry.integrity_warnings(),
        ["i18n.bundle_integrity_failed"]
    );
    Ok(())
}
