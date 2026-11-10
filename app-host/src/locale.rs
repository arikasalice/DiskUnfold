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
    let tags = du_platform_os::windows_ui_language::preferred_ui_languages().ok();
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
    let korean_integrity_failed = registry
        .integrity_warnings()
        .iter()
        .any(|warning| warning == "i18n.bundle_integrity_failed");
    let registered: Vec<_> = registry.languages().collect();
    let decision = locale::decide(&locale::Inputs {
        registered: &registered,
        stored,
        os,
        korean_integrity_failed,
    })
    .map_err(|error| io::Error::other(error.reason_key()))?;
    for warning in registry.integrity_warnings() {
        eprintln!("{warning}");
    }
    Ok(StartupLocale {
        _registry: registry,
        _decision: decision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let startup = initialize(registry, Some("ko")).map_err(|error| error.to_string())?;
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
}
