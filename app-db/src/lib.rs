//! Implement SQLite storage ports.

use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

mod history_schema;
mod job_history;
pub mod migration;
pub mod recent_scope;
mod recent_scope_schema;
pub mod ruleset;
mod settings;

pub use history_schema::{HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION};
pub use job_history::read as history_read;

const APPLICATION_ID: u32 = 0x4455_4442;
const SCHEMA_VERSION: u32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// A configured app database. Feature-owned repositories keep SQL inside this adapter.
pub struct AppDb {
    connection: Connection,
}

#[derive(Debug)]
pub enum OpenError {
    Sqlite(rusqlite::Error),
    ForeignDatabase,
    UnsupportedSchema(u32),
    MigrationRequired,
    HistorySchema(Box<migration::MigrationError>),
    Io(std::io::Error),
    WalUnavailable,
    /// The initialized database was published. Retry by opening it, not creating it.
    CreatedButOpenFailed(Box<OpenError>),
}

impl OpenError {
    /// Returns whether SQLite identified the file as corrupt or not a database.
    ///
    /// This classification lets startup adapters distinguish damaged database bytes from
    /// unrelated failures such as a missing or unwritable data directory.
    pub fn is_corrupt_database(&self) -> bool {
        match self {
            Self::Sqlite(error) => sqlite_reports_corruption(error),
            Self::HistorySchema(error) => matches!(
                error.as_ref(),
                migration::MigrationError::Sqlite(error) if sqlite_reports_corruption(error)
            ),
            Self::CreatedButOpenFailed(error) => error.is_corrupt_database(),
            _ => false,
        }
    }
}

fn sqlite_reports_corruption(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(error, _)
            if matches!(
                error.code,
                rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
            )
    )
}

/// Result of an explicit WAL truncate checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalCheckpoint {
    /// WAL frames observed by SQLite, or `None` when no WAL file existed.
    pub log_frames: Option<i64>,
    /// Frames copied into the database file, or `None` when no WAL file existed.
    pub checkpointed_frames: Option<i64>,
}

/// Failure to close an app database while preserving its open connection.
pub struct AppDbCloseError {
    /// The connection that remains open and can be retried after releasing statements.
    pub database: AppDb,
    /// SQLite's close error.
    pub error: rusqlite::Error,
}

impl std::fmt::Debug for AppDbCloseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppDbCloseError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for AppDbCloseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "database connection close failed: {}", self.error)
    }
}

impl std::error::Error for AppDbCloseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Failure to complete an explicit WAL checkpoint.
#[derive(Debug)]
pub enum WalCheckpointError {
    /// SQLite could not read or checkpoint the WAL.
    Sqlite(rusqlite::Error),
    /// Another connection prevented SQLite from completing the truncate checkpoint.
    Busy {
        /// WAL frames observed when SQLite reported contention.
        log_frames: i64,
        /// WAL frames copied before SQLite reported contention.
        checkpointed_frames: i64,
    },
    /// SQLite returned an invalid negative checkpoint count.
    InvalidCounts {
        /// WAL frames observed by SQLite.
        log_frames: i64,
        /// Frames copied into the database file.
        checkpointed_frames: i64,
    },
}

impl std::fmt::Display for WalCheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "WAL checkpoint failed: {error}"),
            Self::Busy {
                log_frames,
                checkpointed_frames,
            } => write!(
                f,
                "WAL checkpoint was busy after copying {checkpointed_frames} of {log_frames} frames"
            ),
            Self::InvalidCounts {
                log_frames,
                checkpointed_frames,
            } => write!(
                f,
                "WAL checkpoint returned invalid frame counts ({log_frames}, {checkpointed_frames})"
            ),
        }
    }
}

impl std::error::Error for WalCheckpointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Busy { .. } | Self::InvalidCounts { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for OpenError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl AppDb {
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.pragma_update(None, "foreign_keys", true)?;

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let application_id: u32 =
            transaction.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let schema: u32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match application_id {
            APPLICATION_ID => {}
            0 if schema == 0 => {
                let has_objects: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_schema)",
                    [],
                    |row| row.get(0),
                )?;
                if has_objects {
                    return Err(OpenError::ForeignDatabase);
                }
                transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            }
            _ => return Err(OpenError::ForeignDatabase),
        }
        if schema != 0 {
            return Err(OpenError::UnsupportedSchema(schema));
        }
        transaction.commit()?;

        let journal: String =
            connection.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !journal.eq_ignore_ascii_case("wal") {
            return Err(OpenError::WalUnavailable);
        }
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self { connection })
    }

    /// Read without creating a file, changing pragmas or running migrations.
    pub fn open_read_only(path: &Path) -> Result<Self, OpenError> {
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if HISTORY_DEFINITIONS
            .steps
            .iter()
            .any(|definition| definition.number == version)
        {
            migration::verify_registered(&mut connection, &HISTORY_DEFINITIONS, false)
                .map_err(history_schema::schema_error)?;
        } else {
            check_existing(&connection, true)?;
        }
        Ok(Self { connection })
    }

    /// Reads only `user_version` from a read-only connection with the normal
    /// five-second busy timeout. This selects an open path; callers must still
    /// validate the database through the connection they will use.
    pub fn probe_schema_version_read_only(path: &Path) -> Result<u32, OpenError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(OpenError::Sqlite)
    }

    /// Open an already-current database for GUI writes. Older databases require
    /// a future backup-and-migration runner and are never modified here.
    pub fn open_gui(path: &Path) -> Result<Self, OpenError> {
        Self::open_gui_checked(path, |connection| check_existing(connection, false))
    }

    fn open_gui_checked(
        path: &Path,
        check: impl FnOnce(&mut Connection) -> Result<(), OpenError>,
    ) -> Result<Self, OpenError> {
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        check(&mut connection)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        // SQLite fires the purge guard on rows removed by REPLACE with this enabled.
        connection.pragma_update(None, "recursive_triggers", true)?;
        let journal: String =
            connection.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !journal.eq_ignore_ascii_case("wal") {
            return Err(OpenError::WalUnavailable);
        }
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self { connection })
    }

    /// Create a new current-format database; never overwrites an existing file.
    pub fn create_gui(path: &Path) -> Result<Self, OpenError> {
        create_gui_with(path, initialize_settings, || Ok(()), Self::open_gui)
    }

    /// Opens the current application database or publishes its initial schema.
    /// If another creator wins the publication race, opens its database in place.
    pub fn open_or_create_gui(path: &Path) -> Result<Self, OpenError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(OpenError::Io)?;
        }
        if path.try_exists().map_err(OpenError::Io)? {
            return Self::open_gui(path);
        }
        match Self::create_gui(path) {
            Err(OpenError::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Self::open_gui(path)
            }
            result => result,
        }
    }

    pub fn schema_version(&self) -> rusqlite::Result<u32> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
    }

    /// Completes a SQLite `TRUNCATE` WAL checkpoint on this connection.
    ///
    /// Call only after application writers have stopped. SQLite's configured
    /// busy timeout bounds lock waiting; contention is returned as `Busy` and
    /// must not be reported as a completed checkpoint. This method does not
    /// consume or close the database connection.
    pub fn checkpoint_wal(&mut self) -> Result<WalCheckpoint, WalCheckpointError> {
        let (busy, log_frames, checkpointed_frames): (i64, i64, i64) = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(WalCheckpointError::Sqlite)?;

        if busy != 0 {
            return Err(WalCheckpointError::Busy {
                log_frames,
                checkpointed_frames,
            });
        }
        if log_frames < -1 || checkpointed_frames < -1 {
            return Err(WalCheckpointError::InvalidCounts {
                log_frames,
                checkpointed_frames,
            });
        }

        Ok(WalCheckpoint {
            log_frames: (log_frames >= 0).then_some(log_frames),
            checkpointed_frames: (checkpointed_frames >= 0).then_some(checkpointed_frames),
        })
    }

    /// Closes this database connection, returning the still-open handle on failure.
    pub fn close(self) -> Result<(), AppDbCloseError> {
        match self.connection.close() {
            Ok(()) => Ok(()),
            Err((connection, error)) => Err(AppDbCloseError {
                database: Self { connection },
                error,
            }),
        }
    }
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct PendingFile {
    path: PathBuf,
    cleaned: bool,
}
impl Drop for PendingFile {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn reserve_temporary(path: &Path) -> Result<PendingFile, OpenError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    loop {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".diskunfold-create-{}-{id}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => {
                return Ok(PendingFile {
                    path: candidate,
                    cleaned: false,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(OpenError::Io(error)),
        }
    }
}

fn create_gui_with(
    path: &Path,
    initialize: impl FnOnce(&Transaction<'_>) -> Result<(), OpenError>,
    before_publish: impl FnOnce() -> Result<(), OpenError>,
    open: impl FnOnce(&Path) -> Result<AppDb, OpenError>,
) -> Result<AppDb, OpenError> {
    let mut pending = reserve_temporary(path)?;
    let mut connection =
        Connection::open_with_flags(&pending.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    initialize(&tx)?;
    tx.commit()?;
    drop(connection);
    before_publish()?;
    fs::hard_link(&pending.path, path).map_err(OpenError::Io)?;
    fs::remove_file(&pending.path)
        .map_err(|error| OpenError::CreatedButOpenFailed(Box::new(OpenError::Io(error))))?;
    pending.cleaned = true;
    open(path).map_err(|error| OpenError::CreatedButOpenFailed(Box::new(error)))
}

fn initialize_settings(tx: &Transaction<'_>) -> Result<(), OpenError> {
    tx.execute_batch(settings::SCHEMA)?;
    tx.execute(
        "UPDATE user_env_setting_version SET schema_version = ?1 WHERE version_key = 'settings'",
        [du_core_contracts::settings::SETTINGS_SCHEMA_VERSION],
    )?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

fn check_existing(connection: &Connection, read_only: bool) -> Result<(), OpenError> {
    let application_id: u32 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id != APPLICATION_ID {
        return Err(OpenError::ForeignDatabase);
    }
    let schema: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match schema {
        SCHEMA_VERSION => Ok(()),
        0 if read_only => Err(OpenError::UnsupportedSchema(0)),
        0 => Err(OpenError::MigrationRequired),
        other => Err(OpenError::UnsupportedSchema(other)),
    }
}

#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod shutdown_tests;
#[cfg(test)]
mod tests;
