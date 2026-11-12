//! Own one shared settings and history writer for the Windows GUI.

use std::{
    collections::BTreeMap,
    fs::File,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use du_app_db::{
    AppDb,
    recent_scope::{
        RecentError, RecentItem, RecentScopeStore, RecentSummary, RecentWrite, list_recent_scopes,
        read_recent_scope,
    },
};
use du_core_contracts::{
    job::{
        HistoryAppend, HistoryAppendOutcome, HistoryInterruptionReceipt, HistoryRecovery,
        JobHistory, JobSummary,
    },
    settings::{
        SettingsChanges, SettingsCommit, SettingsSnapshot, SettingsStore, SettingsStoreError,
    },
};

use du_platform_os::windows_instance_lock::WindowsInstanceLock;

/// One application-owned database connection for settings and history writes.
#[derive(Clone)]
pub(super) struct SharedDatabase(Arc<Database>);

struct Database {
    connection: Mutex<AppDb>,
    _guard: Option<DatabaseGuard>,
}

/// Retains the database path and GUI instance lock until the last writer drops.
pub(super) struct DatabaseGuard {
    _file: File,
    _instance: WindowsInstanceLock,
}

impl DatabaseGuard {
    pub(super) fn new(file: File, instance: WindowsInstanceLock) -> Self {
        Self {
            _file: file,
            _instance: instance,
        }
    }
}

impl SharedDatabase {
    /// Retains the database and its startup guard for every writer.
    pub(super) fn new(connection: AppDb, guard: Option<DatabaseGuard>) -> Self {
        Self(Arc::new(Database {
            connection: Mutex::new(connection),
            _guard: guard,
        }))
    }

    /// A settings port backed by the same connection as the history port.
    pub(super) fn settings(&self) -> SharedSettings {
        SharedSettings(self.clone())
    }

    /// An F24 history port using the same connection and an injected UTC clock.
    pub(super) fn history(&self, now_ms: fn() -> i64) -> SharedHistory {
        SharedHistory {
            database: self.clone(),
            now_ms,
        }
    }

    /// Commits an owner-validated body on the shared Windows GUI connection.
    ///
    /// Preserves the explicitly supplied retention limit used by the opt-in
    /// worker. The remember setting is read in the same write transaction;
    /// the legacy setting adapter defaults to remembering when its value cannot
    /// be read.
    pub(super) fn put_recent_scope(
        &self,
        cap: NonZeroUsize,
        item: &RecentWrite<'_>,
        stopped: &AtomicBool,
    ) -> Result<(), RecentError> {
        let mut connection = self.0.connection.lock().map_err(|_| RecentError::Storage)?;
        let exact_body = |body: &str| body == item.body;
        RecentScopeStore::new(&mut connection, cap, &exact_body)?.put_with_setting(
            item,
            du_user_env::scope::REMEMBER_RECENT,
            |remember| {
                !stopped.load(Ordering::Acquire)
                    && du_user_env::scope::remember_recent_value(remember)
            },
        )?;
        Ok(())
    }

    /// Reads recent-scope summaries on the shared Windows GUI connection.
    ///
    /// FR8.HD1.app-host.DD2: the caller supplies the physical 500-row read cap,
    /// independent of the cap selected for future writes.
    pub(super) fn recent_scope_summaries(
        &self,
        cap: NonZeroUsize,
    ) -> Result<Vec<RecentSummary>, RecentError> {
        let connection = self.0.connection.lock().map_err(|_| RecentError::Storage)?;
        list_recent_scopes(&connection, cap)
    }

    /// Reads a stored row before the application validates its body and rechecks the scope.
    pub(super) fn recent_scope(&self, id: &[u8; 16]) -> Result<Option<RecentItem>, RecentError> {
        let connection = self.0.connection.lock().map_err(|_| RecentError::Storage)?;
        read_recent_scope(&connection, id)
    }
}

impl HistoryRecovery for SharedDatabase {
    fn mark_interrupted(&self, now_ms: i64) -> Result<Option<HistoryInterruptionReceipt>, String> {
        self.0
            .connection
            .lock()
            .map_err(|_| "history.record_failed".to_owned())?
            .mark_interrupted_jobs(now_ms)
    }
}

/// Settings service adapter. Every operation acquires the shared connection.
pub(super) struct SharedSettings(SharedDatabase);

impl SharedSettings {
    /// Applies the registered additive-only transition under the GUI startup lock.
    pub(super) fn advance_schema(&mut self) -> Result<Option<u32>, SettingsStoreError> {
        self.0
            .0
            .connection
            .lock()
            .map_err(|_| SettingsStoreError::Storage)?
            .advance_empty_settings_schema(du_user_env::settings::EMPTY_SCHEMA_TRANSITIONS)
    }

    /// Reads the current settings header without loading or changing setting rows.
    pub(super) fn load_schema_header_for_restore(
        &mut self,
    ) -> Result<(u64, u32), SettingsStoreError> {
        self.0
            .0
            .connection
            .lock()
            .map_err(|_| SettingsStoreError::Storage)?
            .load_settings_schema_header_for_restore()
    }

    /// Loads raw rows for startup restoration without opening another connection.
    pub(super) fn load_for_restore(
        &mut self,
    ) -> Result<(u64, u32, BTreeMap<String, String>), SettingsStoreError> {
        self.0
            .0
            .connection
            .lock()
            .map_err(|_| SettingsStoreError::Storage)?
            .load_settings_for_restore()
    }

    /// Quarantines invalid rows using the same shared connection.
    pub(super) fn quarantine(
        &mut self,
        revision: u64,
        invalid: &BTreeMap<String, String>,
        now_ms: i64,
    ) -> Result<u64, SettingsStoreError> {
        self.0
            .0
            .connection
            .lock()
            .map_err(|_| SettingsStoreError::Storage)?
            .quarantine_settings(revision, invalid, now_ms)
    }
}

impl SettingsStore for SharedSettings {
    fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
        self.0
            .0
            .connection
            .lock()
            .map_err(|_| SettingsStoreError::Storage)?
            .load()
    }
    fn commit(
        &mut self,
        revision: u64,
        changes: &SettingsChanges,
        context: SettingsCommit,
    ) -> Result<u64, SettingsStoreError> {
        self.0
            .0
            .connection
            .lock()
            .map_err(|_| SettingsStoreError::Storage)?
            .commit(revision, changes, context)
    }
}

/// F24 history adapter; F21 linkage is written by `AppDb` in one transaction.
pub(super) struct SharedHistory {
    database: SharedDatabase,
    now_ms: fn() -> i64,
}

impl JobHistory for SharedHistory {
    fn append(&self, summary: &JobSummary) -> Result<HistoryAppend, String> {
        self.append_with_receipt(summary)
            .map(HistoryAppendOutcome::status)
    }

    fn append_with_receipt(&self, summary: &JobSummary) -> Result<HistoryAppendOutcome, String> {
        let now_ms = (self.now_ms)();
        self.database
            .0
            .connection
            .lock()
            .map_err(|_| "history.record_failed".to_owned())?
            .append_scan_history_with_receipt(summary, now_ms)
            .map_err(|reason| {
                eprintln!("history.record_failed: {reason}");
                "history.record_failed".to_owned()
            })
    }
}

#[cfg(test)]
mod tests;
