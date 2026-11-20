//! Compose stored desktop settings and locale profile commands.

use std::{
    io,
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use du_core_contracts::size_units::{IEC_UNITS, SI_UNITS};
use du_engine_api::{
    desktop::SharedTableLimitSource, size_rule::SizeRules, viz_production::UnitTableSource,
};
use du_user_env::space::restore_shared_table_limit;
use du_user_env::{
    Registry, locale,
    settings::{RestoreStatus, SettingsService, builtin_descriptors_with_size_rules},
};

use crate::shared_database::{SharedDatabase, SharedSettings};

#[cfg(windows)]
mod display_ipc;
mod locale_ipc;
#[cfg(windows)]
mod restore_ipc;
mod size_rule;
mod startup_memory_ipc;
pub(super) use locale_ipc::LocaleRuntime;

/// Owns the restored service and immutable startup report for the desktop lifetime.
pub(super) struct StartupSettings {
    service: Mutex<SettingsService<SharedSettings>>,
    status: RestoreStatus,
    /// Rule IDs and scan execution share this assembled size rule registry.
    size_rule_ids: Vec<String>,
    size_rules: SizeRules,
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
        let size_rules = size_rule::assemble()?;
        let size_rule_ids = size_rule::registered_ids(&size_rules);
        Self::open_with_registration(
            database,
            timestamp_ms,
            configured_languages,
            size_rule_ids,
            size_rules,
        )
    }

    /// Opens the settings with the size rule IDs the composition root registered.
    #[cfg(all(test, windows))]
    pub(super) fn open_with_size_rule_ids(
        database: &SharedDatabase,
        timestamp_ms: i64,
        configured_languages: &[&str],
        size_rule_ids: Vec<String>,
    ) -> Result<Self, io::Error> {
        let size_rules = size_rule::assemble()?;
        Self::open_with_registration(
            database,
            timestamp_ms,
            configured_languages,
            size_rule_ids,
            size_rules,
        )
    }

    fn open_with_registration(
        database: &SharedDatabase,
        timestamp_ms: i64,
        configured_languages: &[&str],
        size_rule_ids: Vec<String>,
        size_rules: SizeRules,
    ) -> Result<Self, io::Error> {
        let service =
            restore_settings(database, timestamp_ms, configured_languages, &size_rule_ids)?;
        let status = service.restore_status().clone();
        Ok(Self {
            service: Mutex::new(service),
            status,
            size_rule_ids,
            size_rules,
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

    /// Reads the current policy, failing closed if the settings lock is poisoned.
    pub(super) fn history_retention_policy(
        &self,
    ) -> du_core_contracts::history_retention_ipc::HistoryRetentionPolicy {
        self.service
            .lock()
            .map(|service| service.history_retention_policy())
            .unwrap_or_else(|_| du_user_env::history_retention::unreadable_policy())
    }

    /// Supplies the current policy whenever the GUI reads it.
    pub(super) fn history_retention_policy_source(
        self: Arc<Self>,
    ) -> Arc<
        dyn Fn() -> du_core_contracts::history_retention_ipc::HistoryRetentionPolicy + Send + Sync,
    > {
        Arc::new(move || self.history_retention_policy())
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

    /// Supplies the unit table the display setting selects when a request is admitted.
    ///
    /// An unreadable setting uses the system this platform registers as its default.
    pub(super) fn unit_table_source(self: Arc<Self>) -> UnitTableSource {
        Arc::new(move || {
            let selected = self
                .service
                .lock()
                .ok()
                .and_then(|service| service.display_units().map(|units| units.system == "iec"));
            if selected.unwrap_or(cfg!(windows)) {
                &IEC_UNITS
            } else {
                &SI_UNITS
            }
        })
    }
}

fn restore_settings(
    database: &SharedDatabase,
    timestamp_ms: i64,
    configured_languages: &[&str],
    size_rule_ids: &[String],
) -> Result<SettingsService<SharedSettings>, io::Error> {
    let descriptors = builtin_descriptors_with_size_rules(configured_languages, size_rule_ids)
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

#[cfg(all(test, windows))]
mod tests;
