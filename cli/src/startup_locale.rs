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

mod os_language;
pub(super) use os_language::preferred_ui_languages;

mod stored;
pub(super) use stored::stored_language;
#[cfg(all(test, any(windows, target_os = "linux", target_os = "macos")))]
pub(crate) use stored::{language_from_snapshot, stored_language_in_directory};

#[cfg(test)]
mod macos_tests;
#[cfg(all(test, any(windows, target_os = "linux", target_os = "macos")))]
mod stored_tests;
#[cfg(test)]
mod tests;
