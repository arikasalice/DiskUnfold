//! Read-only access to the stored display language in the application's settings database.
//!
//! The CLI never creates, repairs or migrates the database. A missing directory or database is
//! "nothing stored"; every other failure is reported as `Err(())` so the caller shows one
//! warning line and falls back to the operating-system language.

use du_user_env::locale;

/// Reads the stored display language from this platform's application data directory.
///
/// The directory follows `DISKUNFOLD_DATA_DIR` when set and the per-user platform location
/// otherwise, exactly as the GUI resolves it. `Ok(None)` means no language is stored.
#[cfg(windows)]
pub(crate) fn stored_language() -> Result<Option<String>, ()> {
    let path = std::path::absolute(du_platform_os::application_data::directory().ok_or(())?)
        .map_err(|_| ())?;
    stored_language_in_directory(&path)
}

/// Reads the stored display language from `path` without creating or modifying any file.
///
/// A missing directory or database yields `Ok(None)`. A directory or database that cannot be
/// pinned or opened read-only, or a schema other than the current one, yields `Err(())`.
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

/// Reads the stored display language from this platform's application data directory.
///
/// The directory follows `DISKUNFOLD_DATA_DIR` when set and the per-user platform location
/// otherwise (`$XDG_DATA_HOME` or `~/.local/share` on Linux, `~/Library/Application Support`
/// on macOS), exactly as the GUI resolves it. `Ok(None)` means no language is stored.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn stored_language() -> Result<Option<String>, ()> {
    let directory = du_platform_os::application_data::directory().ok_or(())?;
    stored_language_in_directory(&directory)
}

/// Reads the stored display language from `directory` without creating or modifying any file.
///
/// A missing directory or database yields `Ok(None)`. A database that cannot be opened
/// read-only (corrupt, needing WAL recovery, or of another schema version) yields `Err(())`.
/// This path does not take the macOS or Linux GUI instance lock, because acquiring it creates
/// the lock sentinel file.
#[cfg(any(target_os = "linux", target_os = "macos"))]
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

/// Extracts the display language from a settings snapshot of the current schema version.
///
/// An absent or null value is "nothing stored"; a non-string value or another schema version is
/// an error.
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
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

/// Platforms without a settings reader have no stored language.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(crate) fn stored_language() -> Result<Option<String>, ()> {
    Ok(None)
}
