//! Own one shared settings and history writer for a desktop GUI.

use std::{
    collections::BTreeMap,
    fs::File,
    sync::{Arc, Mutex},
};
#[cfg(windows)]
use std::{
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(windows)]
use du_app_db::recent_scope::{
    RecentError, RecentItem, RecentScopeStore, RecentSummary, RecentWrite, RecentWriteError,
    list_recent_scopes, read_recent_scope,
};
use du_app_db::{AppDb, AppDbCloseError};
use du_app_db::{WalCheckpoint, WalCheckpointError};
#[cfg(windows)]
use du_core_contracts::job::{
    HistoryAppend, HistoryAppendOutcome, HistoryInterruptionReceipt, HistoryRecovery, JobHistory,
    JobSummary,
};
use du_core_contracts::settings::{
    SettingsChanges, SettingsCommit, SettingsSnapshot, SettingsStore, SettingsStoreError,
};
#[cfg(target_os = "linux")]
use du_platform_os::linux_instance_lock::LinuxInstanceLock;
#[cfg(windows)]
use du_platform_os::windows_instance_lock::WindowsInstanceLock;

#[allow(dead_code)]
enum InstanceGuard {
    #[cfg(windows)]
    Windows(WindowsInstanceLock),
    #[cfg(target_os = "linux")]
    Linux(LinuxInstanceLock),
}

/// One application-owned database connection for settings and history writes.
#[derive(Clone)]
pub(super) struct SharedDatabase(Arc<Database>);

/// Failure to take the shared writer lock or finish its WAL checkpoint.
#[derive(Debug)]
pub(super) enum SharedDatabaseCheckpointError {
    /// The shared connection mutex was poisoned.
    ConnectionPoisoned,
    /// SQLite did not complete the truncate checkpoint.
    Wal(WalCheckpointError),
}

impl std::fmt::Display for SharedDatabaseCheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionPoisoned => f.write_str("shared database connection is poisoned"),
            Self::Wal(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SharedDatabaseCheckpointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ConnectionPoisoned => None,
            Self::Wal(error) => Some(error),
        }
    }
}

/// A close refusal keeps the database handle available for release or retry.
#[allow(dead_code)] // Used when the lifecycle assembly connects the final owner port.
pub(super) enum SharedDatabaseCloseError {
    /// Another subsystem still owns this database and must be retired first.
    StillReferenced {
        /// The handle whose ownership was not consumed.
        database: SharedDatabase,
        /// Number of owners observed by the failed unique-owner check.
        owners: usize,
    },
    /// The connection mutex was poisoned; the connection remains open.
    ConnectionPoisoned(SharedDatabase),
    /// The connection remains open because its WAL checkpoint did not finish.
    Checkpoint {
        /// The handle whose checkpoint must be retried or diagnosed.
        database: SharedDatabase,
        /// SQLite's checkpoint failure.
        error: WalCheckpointError,
    },
    /// SQLite refused to close the last connection; the handle remains available.
    DatabaseClose {
        /// The handle that was returned by SQLite on close failure.
        database: SharedDatabase,
        /// SQLite's close error.
        error: rusqlite::Error,
    },
}

impl SharedDatabaseCloseError {
    /// Recovers the still-open handle from a refused close.
    #[allow(dead_code)] // The shutdown coordinator is intentionally not registered here.
    pub(super) fn into_database(self) -> SharedDatabase {
        match self {
            Self::StillReferenced { database, .. }
            | Self::ConnectionPoisoned(database)
            | Self::Checkpoint { database, .. }
            | Self::DatabaseClose { database, .. } => database,
        }
    }
}

impl std::fmt::Debug for SharedDatabaseCloseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StillReferenced { owners, .. } => f
                .debug_struct("StillReferenced")
                .field("owners", owners)
                .finish(),
            Self::ConnectionPoisoned(_) => f.write_str("ConnectionPoisoned"),
            Self::Checkpoint { error, .. } => f.debug_tuple("Checkpoint").field(error).finish(),
            Self::DatabaseClose { error, .. } => {
                f.debug_tuple("DatabaseClose").field(error).finish()
            }
        }
    }
}

impl std::fmt::Display for SharedDatabaseCloseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StillReferenced { owners, .. } => {
                write!(f, "database still has {owners} owners")
            }
            Self::ConnectionPoisoned(_) => f.write_str("shared database connection is poisoned"),
            Self::Checkpoint { error, .. } => write!(f, "{error}"),
            Self::DatabaseClose { error, .. } => {
                write!(f, "database connection close failed: {error}")
            }
        }
    }
}

impl std::error::Error for SharedDatabaseCloseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StillReferenced { .. } | Self::ConnectionPoisoned(_) => None,
            Self::Checkpoint { error, .. } => Some(error),
            Self::DatabaseClose { error, .. } => Some(error),
        }
    }
}

struct Database {
    connection: Mutex<AppDb>,
    _guard: Option<DatabaseGuard>,
}

/// Retains the database path and GUI instance lock until the last writer drops.
pub(super) struct DatabaseGuard {
    _file: File,
    _instance: InstanceGuard,
}

impl DatabaseGuard {
    #[cfg(windows)]
    pub(super) fn new(file: File, instance: WindowsInstanceLock) -> Self {
        Self {
            _file: file,
            _instance: InstanceGuard::Windows(instance),
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn new(file: File, instance: LinuxInstanceLock) -> Self {
        Self {
            _file: file,
            _instance: InstanceGuard::Linux(instance),
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

    /// Serializes a WAL checkpoint with callers using this shared connection.
    ///
    /// The caller must close write admission before invoking this method. It
    /// does not close the connection or prove that the last shared owner has
    /// dropped; every reader and writer must be retired separately.
    pub(super) fn checkpoint_wal(&self) -> Result<WalCheckpoint, SharedDatabaseCheckpointError> {
        let mut connection = self
            .0
            .connection
            .lock()
            .map_err(|_| SharedDatabaseCheckpointError::ConnectionPoisoned)?;
        connection
            .checkpoint_wal()
            .map_err(SharedDatabaseCheckpointError::Wal)
    }

    /// Checkpoints WAL and closes the application database only for its last owner.
    ///
    /// On any refusal, the returned error retains the still-open database handle.
    /// Successful return means the WAL checkpoint completed, the SQLite
    /// connection was dropped, and its database/instance guard was released.
    pub(super) fn close(self) -> Result<WalCheckpoint, SharedDatabaseCloseError> {
        let database = match Arc::try_unwrap(self.0) {
            Ok(database) => database,
            Err(shared) => {
                let owners = Arc::strong_count(&shared);
                return Err(SharedDatabaseCloseError::StillReferenced {
                    database: SharedDatabase(shared),
                    owners,
                });
            }
        };
        let Database { connection, _guard } = database;
        let mut connection = match connection.into_inner() {
            Ok(connection) => connection,
            Err(poisoned) => {
                let database = Database {
                    connection: Mutex::new(poisoned.into_inner()),
                    _guard,
                };
                return Err(SharedDatabaseCloseError::ConnectionPoisoned(
                    SharedDatabase(Arc::new(database)),
                ));
            }
        };
        let checkpoint = match connection.checkpoint_wal() {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                let database = Database {
                    connection: Mutex::new(connection),
                    _guard,
                };
                return Err(SharedDatabaseCloseError::Checkpoint {
                    database: SharedDatabase(Arc::new(database)),
                    error,
                });
            }
        };
        if let Err(AppDbCloseError { database, error }) = connection.close() {
            let database = Database {
                connection: Mutex::new(database),
                _guard,
            };
            return Err(SharedDatabaseCloseError::DatabaseClose {
                database: SharedDatabase(Arc::new(database)),
                error,
            });
        }
        drop(_guard);
        Ok(checkpoint)
    }

    /// A settings port backed by the same connection as the history port.
    pub(super) fn settings(&self) -> SharedSettings {
        SharedSettings(self.clone())
    }

    #[cfg(windows)]
    /// An F24 history port using the same connection and an injected UTC clock.
    pub(super) fn history(&self, now_ms: fn() -> i64) -> SharedHistory {
        SharedHistory {
            database: self.clone(),
            now_ms,
        }
    }

    #[cfg(windows)]
    /// Commits an owner-validated body on the shared Windows GUI connection.
    ///
    /// Preserves the explicitly supplied retention limit used by the opt-in
    /// worker. The remember setting is read in the same write transaction;
    /// actual SQL read failures are propagated to the worker for classification.
    pub(super) fn put_recent_scope(
        &self,
        cap: NonZeroUsize,
        item: &RecentWrite<'_>,
        stopped: &AtomicBool,
    ) -> Result<(), RecentWriteError> {
        let mut connection = self
            .0
            .connection
            .lock()
            .map_err(|_| RecentWriteError::Other(RecentError::Storage))?;
        let exact_body = |body: &str| body == item.body;
        RecentScopeStore::new(&mut connection, cap, &exact_body)
            .map_err(RecentWriteError::Other)?
            .put_with_setting_checked_staged(
                item,
                du_user_env::scope::REMEMBER_RECENT,
                |remember| {
                    !stopped.load(Ordering::Acquire)
                        && du_user_env::scope::remember_recent_value(remember)
                },
            )?;
        Ok(())
    }

    /// Resolves whether one recent-scope attempt is currently enabled.
    ///
    /// This deferred read snapshot runs before native observation and encoding,
    /// so an independent writer does not turn a committed off value into a busy
    /// warning. The final write still rechecks the setting in its immediate
    /// transaction with insertion and eviction.
    #[cfg(windows)]
    pub(super) fn resolve_recent_scope_remember(
        &self,
        cap: NonZeroUsize,
        stopped: &AtomicBool,
    ) -> Result<bool, RecentError> {
        if stopped.load(Ordering::Acquire) {
            return Ok(false);
        }
        let mut connection = self.0.connection.lock().map_err(|_| RecentError::Storage)?;
        let accept_body = |_body: &str| true;
        RecentScopeStore::new(&mut connection, cap, &accept_body)?.resolve_with_setting(
            du_user_env::scope::REMEMBER_RECENT,
            |value| {
                !stopped.load(Ordering::Acquire) && du_user_env::scope::remember_recent_value(value)
            },
        )
    }

    /// Reads recent-scope summaries on the shared Windows GUI connection.
    ///
    /// FR8.HD1.app-host.DD2: the caller supplies the physical 500-row read cap,
    /// independent of the cap selected for future writes.
    #[cfg(windows)]
    pub(super) fn recent_scope_summaries(
        &self,
        cap: NonZeroUsize,
    ) -> Result<Vec<RecentSummary>, RecentError> {
        let connection = self.0.connection.lock().map_err(|_| RecentError::Storage)?;
        list_recent_scopes(&connection, cap)
    }

    #[cfg(windows)]
    /// Reads a stored row before the application validates its body and rechecks the scope.
    pub(super) fn recent_scope(&self, id: &[u8; 16]) -> Result<Option<RecentItem>, RecentError> {
        let connection = self.0.connection.lock().map_err(|_| RecentError::Storage)?;
        read_recent_scope(&connection, id)
    }
}

#[cfg(windows)]
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
#[cfg(windows)]
pub(super) struct SharedHistory {
    database: SharedDatabase,
    now_ms: fn() -> i64,
}

#[cfg(windows)]
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

#[cfg(all(test, windows))]
mod tests;
