//! Open an existing current database for one CLI history append.

use std::{fs, path::Path};

use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::{
    APPLICATION_ID, AppDb, BUSY_TIMEOUT, HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION, OpenError,
    migration,
};

/// A reason an existing file is not eligible for a CLI history append.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryOpenRefusal {
    /// The application database does not exist.
    MissingDatabase,
    /// An incomplete creation or migration marker requires GUI recovery.
    MigrationInProgress,
    /// The file belongs to another application.
    ForeignDatabase,
    /// The database is not at the exact history schema version.
    UnsupportedSchema(u32),
    /// The database is not already in WAL mode.
    NotWal,
    /// The registered schema at the current version failed validation.
    InvalidSchema,
}

/// Opening an existing database for CLI history failed.
#[derive(Debug)]
pub enum HistoryOpenError {
    /// The database was refused without changing it.
    Refused(HistoryOpenRefusal),
    /// Another SQLite writer held a lock through the configured busy timeout.
    Busy,
    /// SQLite reported that the storage device is full.
    DiskFull,
    /// An I/O or database error outside the refusal, busy, and disk-full cases.
    Other(OpenError),
}

impl AppDb {
    /// Opens an already-existing current-schema WAL database for a CLI history append.
    ///
    /// This never creates, migrates, repairs, or changes journal mode. The caller
    /// must append only after its job has ended and close the connection promptly.
    pub fn open_existing_for_history(path: &Path) -> Result<Self, HistoryOpenError> {
        if !path
            .try_exists()
            .map_err(|error| HistoryOpenError::Other(OpenError::Io(error)))?
        {
            return Err(HistoryOpenError::Refused(
                HistoryOpenRefusal::MissingDatabase,
            ));
        }
        refuse_history_migration(path).map_err(classify)?;

        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|error| classify(OpenError::Sqlite(error)))?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(|error| classify(OpenError::Sqlite(error)))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|error| classify(OpenError::Sqlite(error)))?;
        connection
            .pragma_update(None, "recursive_triggers", true)
            .map_err(|error| classify(OpenError::Sqlite(error)))?;

        let application_id: u32 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(|error| classify(OpenError::Sqlite(error)))?;
        if application_id != APPLICATION_ID {
            return Err(HistoryOpenError::Refused(
                HistoryOpenRefusal::ForeignDatabase,
            ));
        }
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| classify(OpenError::Sqlite(error)))?;
        if version != HISTORY_SCHEMA_VERSION {
            return Err(HistoryOpenError::Refused(
                HistoryOpenRefusal::UnsupportedSchema(version),
            ));
        }
        let journal: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(|error| classify(OpenError::Sqlite(error)))?;
        if !journal.eq_ignore_ascii_case("wal") {
            return Err(HistoryOpenError::Refused(HistoryOpenRefusal::NotWal));
        }
        migration::verify_registered(&mut connection, &HISTORY_DEFINITIONS, true)
            .map_err(|error| classify(OpenError::HistorySchema(Box::new(error))))?;
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(|error| classify(OpenError::Sqlite(error)))?;

        Ok(Self {
            connection,
            history_commit_path: Some(path.to_owned()),
        })
    }
}

pub(crate) fn refuse_history_migration(path: &Path) -> Result<(), OpenError> {
    crate::refuse_incomplete(path)?;
    let backup_directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .join("db-backups");
    for marker in [
        migration::managed::JOURNAL.to_owned(),
        format!("{}.tmp", migration::managed::JOURNAL),
    ] {
        match fs::symlink_metadata(backup_directory.join(marker)) {
            Ok(_) => return Err(OpenError::IncompleteCreation),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) => {}
            Err(error) => return Err(OpenError::Io(error)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn classify(error: OpenError) -> HistoryOpenError {
    match error {
        OpenError::ForeignDatabase => {
            HistoryOpenError::Refused(HistoryOpenRefusal::ForeignDatabase)
        }
        OpenError::UnsupportedSchema(version) => {
            HistoryOpenError::Refused(HistoryOpenRefusal::UnsupportedSchema(version))
        }
        OpenError::MigrationRequired => {
            HistoryOpenError::Refused(HistoryOpenRefusal::UnsupportedSchema(0))
        }
        OpenError::WalUnavailable => HistoryOpenError::Refused(HistoryOpenRefusal::NotWal),
        OpenError::IncompleteCreation => {
            HistoryOpenError::Refused(HistoryOpenRefusal::MigrationInProgress)
        }
        OpenError::Sqlite(error) => match error {
            rusqlite::Error::SqliteFailure(failure, _)
                if matches!(
                    failure.code,
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                ) =>
            {
                HistoryOpenError::Busy
            }
            rusqlite::Error::SqliteFailure(failure, _) if failure.code == ErrorCode::DiskFull => {
                HistoryOpenError::DiskFull
            }
            error => HistoryOpenError::Other(OpenError::Sqlite(error)),
        },
        OpenError::HistorySchema(error) => match *error {
            migration::MigrationError::Sqlite(error) => classify(OpenError::Sqlite(error)),
            _ => HistoryOpenError::Refused(HistoryOpenRefusal::InvalidSchema),
        },
        OpenError::CreatedButOpenFailed(error) => classify(*error),
        other => HistoryOpenError::Other(other),
    }
}
