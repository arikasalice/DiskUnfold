//! Read retention candidates through a separate live, read-only SQLite connection.

use std::{path::Path, sync::Arc};

use du_app_db::AppDb;
use du_core_contracts::job::{
    HistoryExpiryCandidates, HistoryExpiryRead, HistoryExpiryReadError, HistoryPruneCandidate,
    HistoryPruneDelete, HistoryPruneDeleteError, HistoryPruneDeleteReceipt, HistoryPrunePosition,
    HistoryPruneRead, HistoryPruneReadError,
};

use crate::{settings::StartupSettings, shared_database::SharedDatabase};

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

impl HistoryPruneRead for HistoryExpiryReader {
    fn expired_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
        after: Option<HistoryPrunePosition>,
        limit: usize,
    ) -> Result<Vec<HistoryPruneCandidate>, HistoryPruneReadError> {
        self.connection()
            .map_err(|_| HistoryPruneReadError::Unavailable)?
            .read_history_prune_candidates(days, reference_ms, after, limit)
            .map_err(|_| HistoryPruneReadError::Unavailable)
    }
}

/// Applies the current policy guard and deletes one history batch on the shared writer.
pub(super) struct HistoryPruneWriter {
    settings: Arc<StartupSettings>,
    database: SharedDatabase,
}

impl HistoryPruneWriter {
    pub(super) fn new(settings: Arc<StartupSettings>, database: SharedDatabase) -> Self {
        Self { settings, database }
    }
}

impl HistoryPruneDelete for HistoryPruneWriter {
    fn delete_expired(
        &self,
        expected_policy_fingerprint: &str,
        days: u32,
        reference_ms: i64,
        candidates: &[HistoryPruneCandidate],
        observed_at_ms: i64,
    ) -> Result<Option<HistoryPruneDeleteReceipt>, HistoryPruneDeleteError> {
        self.settings
            .with_history_retention_policy(expected_policy_fingerprint, || {
                self.database
                    .0
                    .connection
                    .lock()
                    .map_err(|_| HistoryPruneDeleteError::Unavailable)?
                    .delete_expired_history_batch(candidates, days, reference_ms, observed_at_ms)
            })
    }
}
