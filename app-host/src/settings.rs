//! Compose stored Windows settings into the desktop scan registration.

use std::{
    io,
    path::Path,
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use du_app_db::AppDb;
use du_engine_api::desktop::SharedTableLimitSource;
use du_user_env::{
    Registry,
    settings::{RestoreStatus, SettingsService},
    space::{self, restore_shared_table_limit},
};

/// Reads settings once at startup. Jobs retain a validated value at admission.
pub(crate) fn scan_limit_source() -> Result<SharedTableLimitSource, io::Error> {
    let data_dir = du_platform_os::application_data::directory()
        .ok_or_else(|| io::Error::other("db.open_failed: no Windows application data directory"))?;
    let settings = Arc::new(StartupSettings::open(
        &data_dir.join("diskunfold.db"),
        current_time_ms(),
    )?);
    if let Some(reason) = settings.status.failure_reason {
        eprintln!("{reason}");
    }
    for key in &settings.status.invalid_keys {
        eprintln!("settings.value_invalid: {key}");
    }
    Ok(settings.scan_limit_source())
}

/// Owns the restored service and immutable startup report for the desktop lifetime.
struct StartupSettings {
    service: Mutex<SettingsService<AppDb>>,
    status: RestoreStatus,
}

impl StartupSettings {
    fn open(path: &Path, timestamp_ms: i64) -> Result<Self, io::Error> {
        let service = open_settings(path, timestamp_ms)?;
        let status = service.restore_status().clone();
        Ok(Self {
            service: Mutex::new(service),
            status,
        })
    }

    fn scan_limit_source(self: Arc<Self>) -> SharedTableLimitSource {
        Arc::new(move || {
            self.service
                .lock()
                .ok()
                .and_then(|service| restore_shared_table_limit(service.snapshot()))
                .unwrap_or_default()
        })
    }
}

fn open_settings(path: &Path, timestamp_ms: i64) -> Result<SettingsService<AppDb>, io::Error> {
    let database = AppDb::open_or_create_gui(path)
        .map_err(|error| io::Error::other(format!("db.open_failed: {error:?}")))?;
    let registry = Registry::new(space::descriptors())
        .map_err(|error| io::Error::other(format!("settings.registration_failed: {error:?}")))?;
    Ok(SettingsService::restore(
        registry,
        database,
        AppDb::load_settings_for_restore,
        |database, revision, invalid| database.quarantine_settings(revision, invalid, timestamp_ms),
    ))
}

fn current_time_ms() -> i64 {
    UNIX_EPOCH
        .elapsed()
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
