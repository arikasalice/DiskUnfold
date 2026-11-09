//! Implement SQLite storage ports.

use std::{path::Path, time::Duration};

use rusqlite::{Connection, TransactionBehavior};

const APPLICATION_ID: u32 = 0x4455_4442;
const SCHEMA_VERSION: u32 = 0;
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
    WalUnavailable,
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
        if schema != SCHEMA_VERSION {
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

    pub fn schema_version(&self) -> rusqlite::Result<u32> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
    }
}

#[cfg(test)]
mod tests;
