//! Pre-window language bundle registration and startup integrity reporting.

use std::io;

use du_user_env::{i18n, locale};

/// Keeps the registered resources and selected language for the desktop lifetime.
pub(super) struct StartupLocale {
    _registry: i18n::Registry,
    _decision: locale::Decision,
}

pub(super) fn register() -> Result<i18n::Registry, io::Error> {
    check_registration(i18n::embedded::register(&[]))
}

fn check_registration(
    registration: Result<i18n::Registry, i18n::RegistrationError>,
) -> Result<i18n::Registry, io::Error> {
    registration.map_err(|error| io::Error::other(error.reason_key()))
}

pub(super) fn initialize(
    registry: i18n::Registry,
    stored: Option<&str>,
) -> Result<StartupLocale, io::Error> {
    initialize_with_language_provider(registry, stored, || {
        du_platform_os::windows_ui_language::preferred_ui_languages().ok()
    })
}

fn initialize_with_language_provider(
    registry: i18n::Registry,
    stored: Option<&str>,
    provider: impl FnOnce() -> Option<Vec<String>>,
) -> Result<StartupLocale, io::Error> {
    let tags = if stored.is_some_and(|language| registry.bundle(language).is_some())
        || korean_integrity_failed(&registry)
    {
        None
    } else {
        provider()
    };
    let tags = tags
        .as_ref()
        .map(|tags| tags.iter().map(String::as_str).collect::<Vec<_>>());
    let os = tags.as_deref().map_or(
        locale::OsLanguage::Unavailable,
        locale::OsLanguage::Supported,
    );
    initialize_with_os(registry, stored, os)
}

/// Applies the startup table to one observation without writing settings.
fn initialize_with_os(
    registry: i18n::Registry,
    stored: Option<&str>,
    os: locale::OsLanguage<'_>,
) -> Result<StartupLocale, io::Error> {
    initialize_with_report(registry, stored, os, |warning| eprintln!("{warning}"))
}

/// Selects the startup language before reporting registration warnings.
fn initialize_with_report(
    registry: i18n::Registry,
    stored: Option<&str>,
    os: locale::OsLanguage<'_>,
    mut report: impl FnMut(&str),
) -> Result<StartupLocale, io::Error> {
    let korean_integrity_failed = korean_integrity_failed(&registry);
    let registered: Vec<_> = registry.languages().collect();
    let decision = locale::decide(&locale::Inputs {
        registered: &registered,
        stored,
        os,
        korean_integrity_failed,
    })
    .map_err(|error| io::Error::other(error.reason_key()))?;
    for warning in registry.integrity_warnings() {
        report(warning);
    }
    Ok(StartupLocale {
        _registry: registry,
        _decision: decision,
    })
}

fn korean_integrity_failed(registry: &i18n::Registry) -> bool {
    registry
        .integrity_warnings()
        .iter()
        .any(|warning| warning == "i18n.bundle_integrity_failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{settings::StartupSettings, shared_database::SharedDatabase};
    use du_app_db::AppDb;
    use du_core_contracts::settings::{
        SettingsChanges, SettingsCommit, SettingsSource, SettingsStore,
    };

    const EMPTY_HASH: [u8; 32] = [
        0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc, 0xc9,
        0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca, 0xe4, 0x1f,
        0x32, 0x62,
    ];

    fn resource(language: &'static str, expected_hash: [u8; 32]) -> i18n::BundleResource<'static> {
        i18n::BundleResource {
            language,
            version: "1",
            bytes: b"",
            expected_hash,
        }
    }

    fn parsed_english(_: &[u8]) -> Result<i18n::Bundle, i18n::RegistrationError> {
        Ok(i18n::Bundle {
            language: "en".into(),
            version: "1".into(),
            messages: vec![],
            format_table: serde_json::json!({}),
            plural_rule: "one".into(),
            fallback_language: "en".into(),
        })
    }

    #[test]
    fn embedded_registration_succeeds_and_registration_failure_stops_startup() {
        assert!(
            register()
                .and_then(|registry| initialize(registry, None))
                .is_ok()
        );
        let error = check_registration(Err(i18n::RegistrationError)).err();
        assert_eq!(
            error.map(|error| error.to_string()),
            Some("locale.registration_failed".into())
        );
    }

    #[test]
    fn korean_integrity_failure_selects_english_and_retains_warning() -> Result<(), String> {
        let registry = i18n::Registry::register(
            &[resource("en", EMPTY_HASH), resource("ko", [0; 32])],
            &[],
            &[],
            parsed_english,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            registry.configured_languages().collect::<Vec<_>>(),
            ["en", "ko"]
        );
        let mut os_calls = 0;
        let startup = initialize_with_language_provider(registry, Some("ko"), || {
            os_calls += 1;
            None
        })
        .map_err(|error| error.to_string())?;
        assert_eq!(os_calls, 0);
        assert_eq!(startup._decision.language, "en");
        assert_eq!(startup._decision.source, locale::Source::Baseline);
        assert_eq!(
            startup._registry.integrity_warnings(),
            ["i18n.bundle_integrity_failed"]
        );
        Ok(())
    }

    #[test]
    fn english_integrity_failure_stops_startup() {
        let registry =
            i18n::Registry::register(&[resource("en", [0; 32])], &[], &[], parsed_english);
        assert!(check_registration(registry).is_err());
    }

    #[test]
    fn healthy_bundles_honor_a_stored_korean_override() -> Result<(), io::Error> {
        let startup = initialize(register()?, Some("ko"))?;
        assert_eq!(startup._decision.language, "ko");
        assert_eq!(startup._decision.source, locale::Source::Stored);
        Ok(())
    }

    #[test]
    fn stored_registered_language_does_not_query_the_os() -> Result<(), io::Error> {
        let mut os_calls = 0;
        let startup = initialize_with_language_provider(register()?, Some("ko"), || {
            os_calls += 1;
            None
        })?;
        assert_eq!(os_calls, 0);
        assert_eq!(startup._decision.language, "ko");
        assert_eq!(startup._decision.source, locale::Source::Stored);
        Ok(())
    }

    #[test]
    fn unavailable_stored_language_queries_os_once() -> Result<(), io::Error> {
        let mut calls = 0;
        let startup = initialize_with_language_provider(register()?, Some("fr"), || {
            calls += 1;
            Some(vec!["ko-KR".to_owned()])
        })?;
        assert_eq!(calls, 1);
        assert_eq!(startup._decision.language, "ko");
        assert_eq!(startup._decision.source, locale::Source::Os);
        assert!(startup._decision.stored_unavailable);
        Ok(())
    }

    #[test]
    fn startup_uses_first_os_preference_after_a_stored_override() -> Result<(), io::Error> {
        let os = locale::OsLanguage::Supported(&["ko-KR", "en-US"]);
        let automatic = initialize_with_os(register()?, None, os)?;
        assert_eq!(automatic._decision.language, "ko");
        assert_eq!(automatic._decision.source, locale::Source::Os);
        let stored = initialize_with_os(register()?, Some("en"), os)?;
        assert_eq!(stored._decision.language, "en");
        assert_eq!(stored._decision.source, locale::Source::Stored);
        Ok(())
    }

    #[test]
    fn actual_startup_composition_reports_corruption_and_preserves_stored_override()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("settings.db");
        let mut db = AppDb::open_or_create_gui(&path).map_err(|e| format!("{e:?}"))?;
        db.commit(
            0,
            &SettingsChanges::from([(
                locale::DISPLAY_LANGUAGE_KEY.into(),
                Some(serde_json::json!("ko")),
            )]),
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 1,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
        let db = SharedDatabase::new(db, None);
        let mut resources = i18n::embedded::resources().map_err(|e| format!("{e:?}"))?;
        for resource in &mut resources {
            if resource.language == "ko" {
                resource.expected_hash = [0; 32];
            }
        }
        let declared = i18n::embedded::DECLARED_KEYS
            .iter()
            .map(|key| (*key).to_owned())
            .collect::<Vec<_>>();
        let bundles =
            i18n::Registry::register(&resources, &declared, &[], i18n::embedded::parse_bundle)
                .map_err(|e| format!("{e:?}"))?;
        let configured = bundles.configured_languages().collect::<Vec<_>>();
        let settings = StartupSettings::open(&db, 2, &configured)?;
        let stored = settings.stored_language()?;
        let mut warnings = Vec::new();
        let startup = initialize_with_report(
            bundles,
            stored.as_deref(),
            locale::OsLanguage::Supported(&["ko-KR"]),
            |warning| warnings.push(warning.to_owned()),
        )?;
        assert_eq!(startup._decision.language, "en");
        assert!(startup._registry.bundle("ko").is_none());
        assert_eq!(warnings, ["i18n.bundle_integrity_failed"]);
        assert_eq!(settings.stored_language()?.as_deref(), Some("ko"));
        drop(settings);
        drop(db);
        let mut reopened = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
        let persisted = reopened.load().map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            persisted.values.get(locale::DISPLAY_LANGUAGE_KEY),
            Some(&serde_json::json!("ko"))
        );
        assert_eq!(persisted.revision, 1);
        Ok(())
    }

    #[test]
    fn embedded_startup_honors_os_when_healthy_and_reports_no_warning() -> Result<(), io::Error> {
        let mut warnings = Vec::new();
        let startup = initialize_with_report(
            register()?,
            None,
            locale::OsLanguage::Supported(&["ko-KR"]),
            |warning| warnings.push(warning.to_owned()),
        )?;
        assert_eq!(startup._decision.language, "ko");
        assert!(warnings.is_empty());
        Ok(())
    }

    #[test]
    fn non_integrity_registration_error_stops_startup() -> Result<(), Box<dyn std::error::Error>> {
        let mut resources = i18n::embedded::resources().map_err(|e| format!("{e:?}"))?;
        for resource in &mut resources {
            if resource.language == "ko" {
                resource.version = "mismatched";
            }
        }
        let result = i18n::Registry::register(&resources, &[], &[], i18n::embedded::parse_bundle);
        let error = check_registration(result)
            .err()
            .ok_or("registration succeeded")?;
        assert_eq!(error.to_string(), "locale.registration_failed");
        Ok(())
    }

    #[test]
    fn corrupt_embedded_english_stops_startup() -> Result<(), Box<dyn std::error::Error>> {
        let mut resources = i18n::embedded::resources().map_err(|e| format!("{e:?}"))?;
        for resource in &mut resources {
            if resource.language == "en" {
                resource.expected_hash = [0; 32];
            }
        }
        let result = i18n::Registry::register(&resources, &[], &[], i18n::embedded::parse_bundle);
        let error = check_registration(result)
            .err()
            .ok_or("registration succeeded")?;
        assert_eq!(error.to_string(), "locale.registration_failed");
        Ok(())
    }
}
