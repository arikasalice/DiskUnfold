//! Compose stored Windows settings into the desktop scan registration.

use std::{
    io,
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use du_engine_api::desktop::SharedTableLimitSource;
use du_user_env::{
    Registry, locale,
    settings::{RestoreStatus, SettingsService, builtin_descriptors},
    space::restore_shared_table_limit,
};

use crate::shared_database::{SharedDatabase, SharedSettings};

mod display_ipc;
mod locale_ipc;
mod restore_ipc;
pub(super) use locale_ipc::LocaleRuntime;

/// Owns the restored service and immutable startup report for the desktop lifetime.
pub(super) struct StartupSettings {
    service: Mutex<SettingsService<SharedSettings>>,
    status: RestoreStatus,
}

impl StartupSettings {
    pub(super) fn load(
        database: &SharedDatabase,
        configured_languages: &[&str],
    ) -> Result<Arc<Self>, io::Error> {
        let settings = Self::open(database, current_time_ms(), configured_languages)?;
        if let Some(reason) = settings.status.failure_reason {
            eprintln!("{reason}");
        }
        for key in &settings.status.invalid_keys {
            eprintln!("settings.value_invalid: {key}");
        }
        Ok(Arc::new(settings))
    }

    pub(super) fn open(
        database: &SharedDatabase,
        timestamp_ms: i64,
        configured_languages: &[&str],
    ) -> Result<Self, io::Error> {
        let service = restore_settings(database, timestamp_ms, configured_languages)?;
        let status = service.restore_status().clone();
        Ok(Self {
            service: Mutex::new(service),
            status,
        })
    }

    /// Reads the stored override without rewriting it when a bundle is unavailable.
    pub(super) fn stored_language(&self) -> Result<Option<String>, io::Error> {
        let service = self
            .service
            .lock()
            .map_err(|_| io::Error::other("settings.restore_failed"))?;
        Ok(service
            .snapshot()
            .values
            .get(locale::DISPLAY_LANGUAGE_KEY)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned))
    }

    /// Supplies the validated setting when a scan is admitted.
    pub(super) fn scan_limit_source(self: Arc<Self>) -> SharedTableLimitSource {
        Arc::new(move || {
            self.service
                .lock()
                .ok()
                .and_then(|service| restore_shared_table_limit(service.snapshot()))
                .unwrap_or_default()
        })
    }
}

fn restore_settings(
    database: &SharedDatabase,
    timestamp_ms: i64,
    configured_languages: &[&str],
) -> Result<SettingsService<SharedSettings>, io::Error> {
    let descriptors = builtin_descriptors(configured_languages)
        .map_err(|error| io::Error::other(error.reason_key()))?;
    let registry = Registry::new(descriptors)
        .map_err(|error| io::Error::other(format!("settings.registration_failed: {error:?}")))?;
    let mut store = database.settings();
    let transition = store.advance_schema();
    let failed_migration_observation = if transition.is_err() {
        store.load_schema_header_for_restore().ok()
    } else {
        None
    };
    let migrated_from = transition.as_ref().ok().copied().flatten();
    let mut service = SettingsService::restore(
        registry,
        store,
        |store| match transition {
            Ok(_) => SharedSettings::load_for_restore(store),
            Err(error) => Err(error),
        },
        |database, revision, invalid| database.quarantine(revision, invalid, timestamp_ms),
    );
    if let Some(from) = migrated_from {
        service.record_migration(from);
    }
    if let Some((revision, schema)) = failed_migration_observation {
        service.record_failed_migration_observation(revision, schema);
    }
    Ok(service)
}

pub(super) fn current_time_ms() -> i64 {
    UNIX_EPOCH
        .elapsed()
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
