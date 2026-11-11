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
mod tests;
