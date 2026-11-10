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
    let registered = registry.languages().collect::<Vec<_>>();
    let (stored, tags) = if argument.is_some() {
        (Ok(None), None)
    } else {
        (stored_language(), preferred_ui_languages())
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
        korean_integrity_failed: !registry.integrity_warnings().is_empty(),
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
