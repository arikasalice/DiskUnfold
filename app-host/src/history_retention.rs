//! Read retention candidates through a separate live, read-only SQLite connection.

use std::path::Path;

use du_app_db::AppDb;
use du_core_contracts::job::{HistoryExpiryCandidates, HistoryExpiryRead, HistoryExpiryReadError};

use crate::shared_database::SharedDatabase;

pub(super) struct HistoryExpiryReader {
    path: std::path::PathBuf,
    connection: Option<AppDb>,
    _database: SharedDatabase,
}

impl HistoryExpiryReader {
    /// Defers opening the read connection so a transient read failure reaches the command.
    pub(super) fn new(data_dir: &Path, database: SharedDatabase) -> Self {
        Self {
            path: data_dir.join("diskunfold.db"),
            connection: None,
            _database: database,
        }
    }

    fn connection(&mut self) -> Result<&mut AppDb, HistoryExpiryReadError> {
        if self.connection.is_none() {
            self.connection = Some(
                AppDb::open_read_only_live(&self.path)
                    .map_err(|_| HistoryExpiryReadError::Unavailable)?,
            );
        }
        self.connection
            .as_mut()
            .ok_or(HistoryExpiryReadError::Unavailable)
    }
}

impl HistoryExpiryRead for HistoryExpiryReader {
    fn delete_generation(&mut self) -> Result<u64, HistoryExpiryReadError> {
        self.connection()?
            .read_history_delete_generation()
            .map_err(|_| HistoryExpiryReadError::Unavailable)
    }

    fn expiry_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
    ) -> Result<HistoryExpiryCandidates, HistoryExpiryReadError> {
        self.connection()?
            .read_history_expiry_candidates(days, reference_ms)
            .map_err(|_| HistoryExpiryReadError::Unavailable)
    }
}
