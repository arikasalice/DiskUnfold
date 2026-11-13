//! Read-only composition of the CLI startup language inputs.

use du_user_env::{
    i18n,
    locale::{self, Decision, DecisionError, Inputs, OsLanguage},
};

#[cfg(test)]
fn select_with(
    registry: &i18n::Registry,
    argument: Option<&str>,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
) -> Result<(Decision, bool), DecisionError> {
    select_with_policy(registry, argument, false, read_stored, read_os)
}

/// Selects a CLI language, optionally bypassing the stored settings port.
pub(super) fn select_with_policy(
    registry: &i18n::Registry,
    argument: Option<&str>,
    skip_stored_settings: bool,
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
        let stored = if skip_stored_settings {
            Ok(None)
        } else {
            read_stored()
        };
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
pub(super) fn stored_language() -> Result<Option<String>, ()> {
    let path = std::path::absolute(du_platform_os::application_data::directory().ok_or(())?)
        .map_err(|_| ())?;
    stored_language_in_directory(&path)
}

#[cfg(windows)]
pub(crate) fn stored_language_in_directory(path: &std::path::Path) -> Result<Option<String>, ()> {
    use du_app_db::AppDb;
    use du_core_contracts::settings::SettingsStore;
    use du_platform_os::{
        application_data::DATABASE_FILE, windows_instance_lock::pin_existing_database,
        windows_observation::open_verified_stable,
    };

    if !path.try_exists().map_err(|_| ())? {
        return Ok(None);
    }
    let directory = open_verified_stable(path).map_err(|_| ())?;
    let Some(_pinned) = pin_existing_database(&directory).map_err(|_| ())? else {
        return Ok(None);
    };
    let mut database = AppDb::open_read_only(&path.join(DATABASE_FILE)).map_err(|_| ())?;
    let snapshot = database.load().map_err(|_| ())?;
    language_from_snapshot(&snapshot)
}

#[cfg(target_os = "linux")]
pub(super) fn stored_language() -> Result<Option<String>, ()> {
    let directory = du_platform_os::application_data::directory().ok_or(())?;
    stored_language_in_directory(&directory)
}

#[cfg(target_os = "linux")]
pub(crate) fn stored_language_in_directory(
    directory: &std::path::Path,
) -> Result<Option<String>, ()> {
    use du_app_db::AppDb;
    use du_core_contracts::settings::SettingsStore;
    use du_platform_os::application_data::DATABASE_FILE;

    let database_path = directory.join(DATABASE_FILE);
    if !database_path.try_exists().map_err(|_| ())? {
        return Ok(None);
    }
    let mut database = AppDb::open_read_only(&database_path).map_err(|_| ())?;
    let snapshot = database.load().map_err(|_| ())?;
    language_from_snapshot(&snapshot)
}

#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn language_from_snapshot(
    snapshot: &du_core_contracts::settings::SettingsSnapshot,
) -> Result<Option<String>, ()> {
    if snapshot.schema_version != du_core_contracts::settings::SETTINGS_SCHEMA_VERSION {
        return Err(());
    }
    match snapshot.values.get(locale::DISPLAY_LANGUAGE_KEY) {
        None => Ok(None),
        Some(value) if value.is_null() => Ok(None),
        Some(value) => value.as_str().map(|value| Some(value.to_owned())).ok_or(()),
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn stored_language() -> Result<Option<String>, ()> {
    Ok(None)
}

#[cfg(windows)]
pub(super) fn preferred_ui_languages() -> Option<Vec<String>> {
    du_platform_os::windows_ui_language::preferred_ui_languages().ok()
}

#[cfg(target_os = "linux")]
pub(super) fn preferred_ui_languages() -> Option<Vec<String>> {
    du_platform_os::linux_ui_language::preferred_ui_languages()
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn preferred_ui_languages() -> Option<Vec<String>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn stored_language_reads_linux_settings_read_only_and_warns_for_unregistered_values()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::{
            fs,
            path::PathBuf,
            sync::atomic::{AtomicU64, Ordering},
        };

        use du_core_contracts::settings::{
            SettingsChanges, SettingsCommit, SettingsSource, SettingsStore,
        };

        static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

        struct TempDirectory(PathBuf);

        impl Drop for TempDirectory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        let directory = std::env::temp_dir().join(format!(
            "diskunfold-cli-f13-linux-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory)?;
        let _fixture = TempDirectory(directory.clone());
        let database_path = directory.join(du_platform_os::application_data::DATABASE_FILE);
        assert_eq!(stored_language_in_directory(&directory), Ok(None));
        assert!(!database_path.exists());
        let mut database =
            du_app_db::AppDb::create_gui(&database_path).map_err(|error| format!("{error:?}"))?;
        let language = du_user_env::locale::descriptors(&["en", "ko"])
            .map_err(|error| format!("{error:?}"))?
            .into_iter()
            .find(|descriptor| descriptor.key == locale::DISPLAY_LANGUAGE_KEY)
            .and_then(|descriptor| match descriptor.kind {
                du_user_env::ValueKind::Choice(values) => values
                    .into_iter()
                    .find(|value| value.as_str() == Some("ko")),
                _ => None,
            })
            .ok_or("Korean display language is not registered")?;
        let changes =
            SettingsChanges::from([(locale::DISPLAY_LANGUAGE_KEY.into(), Some(language))]);
        database
            .commit(
                0,
                &changes,
                SettingsCommit {
                    source: SettingsSource::Screen,
                    timestamp_ms: 1,
                },
            )
            .map_err(|error| format!("{error:?}"))?;
        drop(database);

        let before = fs::read(&database_path)?;
        let mut os_reads = 0;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = crate::run_with_io(
            ["scan", "root"].into_iter().map(std::ffi::OsString::from),
            || stored_language_in_directory(&directory),
            || {
                os_reads += 1;
                Some(vec!["en-US".into()])
            },
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(exit, std::process::ExitCode::from(2));
        assert_eq!(os_reads, 0);
        assert!(!String::from_utf8_lossy(&stderr).contains("Saved settings are unavailable"));
        assert_eq!(fs::read(&database_path)?, before);

        let mut database =
            du_app_db::AppDb::open_gui(&database_path).map_err(|error| format!("{error:?}"))?;
        let changes =
            SettingsChanges::from([(locale::DISPLAY_LANGUAGE_KEY.into(), Some("fr".into()))]);
        database
            .commit(
                1,
                &changes,
                SettingsCommit {
                    source: SettingsSource::Screen,
                    timestamp_ms: 2,
                },
            )
            .map_err(|error| format!("{error:?}"))?;
        drop(database);
        let before = fs::read(&database_path)?;
        let mut os_reads = 0;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = crate::run_with_io(
            ["scan", "root"].into_iter().map(std::ffi::OsString::from),
            || stored_language_in_directory(&directory),
            || {
                os_reads += 1;
                Some(vec!["en-US".into()])
            },
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(exit, std::process::ExitCode::from(2));
        assert_eq!(os_reads, 1);
        assert!(
            String::from_utf8_lossy(&stderr)
                .starts_with("Saved settings are unavailable. Using a fallback language.\n")
        );
        assert_eq!(fs::read(&database_path)?, before);
        Ok(())
    }

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
        let (decision, unavailable) =
            select_with_policy(&registry, None, true, || Err(()), || None)
                .map_err(|e| format!("{e:?}"))?;
        assert_eq!(decision.language, "en");
        assert_eq!(decision.source, locale::Source::Baseline);
        assert!(!unavailable);
        Ok(())
    }

    #[test]
    fn explicit_language_skips_both_settings_and_os_lookups()
    -> Result<(), Box<dyn std::error::Error>> {
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
}
