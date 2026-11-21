//! Read terminal history records eligible for expiry from one SQLite snapshot.

use du_core_contracts::job::{HistoryExpiryCandidates, HistoryExpiryRead, HistoryExpiryReadError};
use rusqlite::{TransactionBehavior, params};

use crate::AppDb;

/// Failure to read expiry candidates without changing the requested result.
#[derive(Debug)]
pub enum Error {
    RequiresReadConnection,
    Storage(rusqlite::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequiresReadConnection => {
                formatter.write_str("history expiry queries require a read-only connection")
            }
            Self::Storage(error) => write!(formatter, "history expiry query failed: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RequiresReadConnection => None,
            Self::Storage(error) => Some(error),
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

impl AppDb {
    /// Reads terminal expiry candidates using a deferred read transaction.
    ///
    /// IDs are hashed in byte order while rows are streamed, so memory use does
    /// not grow with the number of histories.
    pub fn read_history_expiry_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
    ) -> Result<HistoryExpiryCandidates, Error> {
        if !self.connection.is_readonly("main")? {
            return Err(Error::RequiresReadConnection);
        }
        let duration_ms = i64::from(days).saturating_mul(86_400_000);
        let cutoff_ms = reference_ms.saturating_sub(duration_ms);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let mut count = 0_u64;
        let mut earliest_reckon_at_ms: Option<i64> = None;
        let mut latest_reckon_at_ms: Option<i64> = None;
        let mut hasher = blake3::Hasher::new();
        {
            let mut statement = tx.prepare(include_str!("expiry_candidates.sql"))?;
            let rows = statement.query_map(params![cutoff_ms, reference_ms], |row| {
                Ok((row.get::<_, [u8; 16]>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (job_id, reckon_at_ms) = row?;
                hasher.update(&job_id);
                count += 1;
                earliest_reckon_at_ms = Some(
                    earliest_reckon_at_ms.map_or(reckon_at_ms, |value| value.min(reckon_at_ms)),
                );
                latest_reckon_at_ms =
                    Some(latest_reckon_at_ms.map_or(reckon_at_ms, |value| value.max(reckon_at_ms)));
            }
        }
        tx.commit()?;
        Ok(HistoryExpiryCandidates {
            count,
            earliest_reckon_at_ms,
            latest_reckon_at_ms,
            digest: *hasher.finalize().as_bytes(),
        })
    }
}

impl HistoryExpiryRead for AppDb {
    fn expiry_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
    ) -> Result<HistoryExpiryCandidates, HistoryExpiryReadError> {
        self.read_history_expiry_candidates(days, reference_ms)
            .map_err(|_| HistoryExpiryReadError::Unavailable)
    }
}

#[cfg(test)]
mod tests;
