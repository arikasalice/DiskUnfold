//! Read terminal history records eligible for expiry from one SQLite snapshot.

use du_core_contracts::{
    Id,
    job::{
        HistoryExpiryCandidate, HistoryExpiryCandidates, HistoryExpiryRead, HistoryExpiryReadError,
    },
};
use rusqlite::{TransactionBehavior, params};

use crate::AppDb;

/// Failure to read expiry candidates without changing the requested result.
#[derive(Debug)]
pub enum Error {
    RequiresReadConnection,
    InvalidJobId,
    Storage(rusqlite::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequiresReadConnection => {
                formatter.write_str("history expiry queries require a read-only connection")
            }
            Self::InvalidJobId => {
                formatter.write_str("history expiry query found an invalid job ID")
            }
            Self::Storage(error) => write!(formatter, "history expiry query failed: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RequiresReadConnection | Self::InvalidJobId => None,
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
    /// Reads the current history deletion generation through a read-only snapshot.
    pub fn read_history_delete_generation(&mut self) -> Result<u64, Error> {
        if !self.connection.is_readonly("main")? {
            return Err(Error::RequiresReadConnection);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let generation = delete_generation(&tx)?;
        tx.commit()?;
        Ok(generation)
    }

    /// Reads terminal expiry candidates using a deferred read transaction.
    ///
    /// The candidate IDs and their metadata are returned in ascending ID order;
    /// the digest and deletion generation come from the same SQLite snapshot.
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
        let delete_generation = delete_generation(&tx)?;
        let mut count = 0_u64;
        let mut earliest_reckon_at_ms: Option<i64> = None;
        let mut latest_reckon_at_ms: Option<i64> = None;
        let mut hasher = blake3::Hasher::new();
        let mut candidates = Vec::new();
        {
            let mut statement = tx.prepare(include_str!("expiry_candidates.sql"))?;
            let rows = statement.query_map(params![cutoff_ms, reference_ms], |row| {
                Ok((
                    row.get::<_, [u8; 16]>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            })?;
            for row in rows {
                let (job_id, reckon_at_ms, has_nonterminal_follow_up) = row?;
                let id = Id::from_v7_bytes(job_id);
                if id.v7_bytes() != Some(job_id) {
                    return Err(Error::InvalidJobId);
                }
                hasher.update(&job_id);
                count += 1;
                earliest_reckon_at_ms = Some(
                    earliest_reckon_at_ms.map_or(reckon_at_ms, |value| value.min(reckon_at_ms)),
                );
                latest_reckon_at_ms =
                    Some(latest_reckon_at_ms.map_or(reckon_at_ms, |value| value.max(reckon_at_ms)));
                candidates.push(HistoryExpiryCandidate {
                    job_id: id,
                    has_nonterminal_follow_up,
                    reckon_at_ms,
                });
            }
        }
        tx.commit()?;
        Ok(HistoryExpiryCandidates {
            delete_generation,
            count,
            earliest_reckon_at_ms,
            latest_reckon_at_ms,
            digest: *hasher.finalize().as_bytes(),
            candidates,
        })
    }
}

fn delete_generation(tx: &rusqlite::Transaction<'_>) -> Result<u64, rusqlite::Error> {
    let generation: i64 = tx.query_row(
        "SELECT delete_generation FROM job_runtime_history_state WHERE state_key='history'",
        [],
        |row| row.get(0),
    )?;
    u64::try_from(generation).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, generation))
}

impl HistoryExpiryRead for AppDb {
    fn delete_generation(&mut self) -> Result<u64, HistoryExpiryReadError> {
        self.read_history_delete_generation()
            .map_err(|_| HistoryExpiryReadError::Unavailable)
    }

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
