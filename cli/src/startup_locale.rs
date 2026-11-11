//! Read-only composition of the CLI startup language inputs.

use du_user_env::{
    i18n,
    locale::{self, Decision, DecisionError, Inputs, OsLanguage},
};

/// Selects the process language and reports an unavailable settings snapshot.
pub(super) fn select(
    registry: &i18n::Registry,
    argument: Option<&str>,
) -> Result<(Decision, bool), DecisionError> {
    select_with(registry, argument, stored_language, preferred_ui_languages)
}

fn select_with(
    registry: &i18n::Registry,
    argument: Option<&str>,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
) -> Result<(Decision, bool), DecisionError> {
    let registered = registry.languages().collect::<Vec<_>>();
    let korean_integrity_failed = registry
        .integrity_warnings()
        .iter()
        .any(|warning| warning == "i18n.bundle_integrity_failed");
    let (stored, tags) = if argument.is_some() {
        (Ok(None), None)
    } else {
        let stored = read_stored();
        let usable = stored
            .as_ref()
            .ok()
            .and_then(|value| value.as_deref())
            .is_some_and(|language| registered.contains(&language));
        let tags = if usable || korean_integrity_failed {
            None
        } else {
            read_os()
        };
        (stored, tags)
    };
    let tags = tags
        .as_ref()
        .map(|tags| tags.iter().map(String::as_str).collect::<Vec<_>>());
    let os = tags
        .as_deref()
        .map_or(OsLanguage::Unavailable, OsLanguage::Supported);
    let inputs = Inputs {
        registered: &registered,
        stored: stored.as_ref().ok().and_then(|value| value.as_deref()),
        os,
        korean_integrity_failed,
    };
    locale::decide_cli(&inputs, argument).map(|decision| (decision, stored.is_err()))
}

#[cfg(windows)]
fn stored_language() -> Result<Option<String>, ()> {
    use du_app_db::AppDb;
    use du_core_contracts::settings::SettingsStore;
    use du_platform_os::{
        application_data::{DATABASE_FILE, directory},
        windows_instance_lock::pin_existing_database,
        windows_observation::open_verified_stable,
    };

    let path = std::path::absolute(directory().ok_or(())?).map_err(|_| ())?;
    if !path.try_exists().map_err(|_| ())? {
        return Ok(None);
    }
    let directory = open_verified_stable(&path).map_err(|_| ())?;
    let Some(_pinned) = pin_existing_database(&directory).map_err(|_| ())? else {
        return Ok(None);
    };
    let mut database = AppDb::open_read_only(&path.join(DATABASE_FILE)).map_err(|_| ())?;
    let snapshot = database.load().map_err(|_| ())?;
    if snapshot.schema_version != 1 {
        return Err(());
    }
    match snapshot.values.get(locale::DISPLAY_LANGUAGE_KEY) {
        None => Ok(None),
        Some(value) if value.is_null() => Ok(None),
        Some(value) => value.as_str().map(|value| Some(value.to_owned())).ok_or(()),
    }
}

#[cfg(not(windows))]
fn stored_language() -> Result<Option<String>, ()> {
    Ok(None)
}

#[cfg(windows)]
fn preferred_ui_languages() -> Option<Vec<String>> {
    du_platform_os::windows_ui_language::preferred_ui_languages().ok()
}

#[cfg(not(windows))]
fn preferred_ui_languages() -> Option<Vec<String>> {
    None
}

#[cfg(test)]
mod tests {
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
}
