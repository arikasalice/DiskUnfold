//! SQLite mechanics for F01 recent scopes.
//!
//! The owner must bind a validated body and fingerprint to an admitted GUI
//! scan, consult the current setting, and supply a published retention limit.
//! This module neither admits scans nor determines the scope body format.

use std::num::NonZeroUsize;

use rusqlite::{ErrorCode, OptionalExtension, TransactionBehavior, params};

use crate::AppDb;

/// Storage errors for the owner to map at its boundary.
#[derive(Debug, PartialEq, Eq)]
pub enum RecentError {
    /// A supplied identifier, limit, count, or body is invalid.
    Invalid,
    /// SQLite could not acquire the required lock.
    Busy,
    /// SQLite reported a full filesystem.
    DiskFull,
    /// A stored value is malformed, unsupported, or exceeds the configured bound.
    Corrupt,
    /// Another database operation failed.
    Storage,
}

/// The summary fields returned by the recent-scope list.
#[derive(Debug, PartialEq, Eq)]
pub struct RecentSummary {
    /// Recent scope identifier.
    pub id: [u8; 16],
    /// Number of included paths.
    pub include_count: u32,
    /// Last use in milliseconds since the Unix epoch.
    pub last_used_at_ms: i64,
}

/// A complete selected row after its body passes the owner's validator.
#[derive(Debug, PartialEq, Eq)]
pub struct RecentItem {
    /// List summary.
    pub summary: RecentSummary,
    /// Persisted scope fingerprint.
    pub fingerprint: [u8; 32],
    /// Owner-validated serialized scope body.
    pub body: String,
}

/// Data bound by the owner to one admitted scan.
pub struct RecentWrite<'a> {
    /// New UUID v7 identifier, used only when inserting.
    pub id: [u8; 16],
    /// Fingerprint of the admitted scope.
    pub fingerprint: [u8; 32],
    /// Serialized scope body.
    pub body: &'a str,
    /// Number of included paths.
    pub include_count: u32,
    /// Use time in milliseconds since the Unix epoch.
    pub now_ms: i64,
}

/// An adapter bound to a database, caller-supplied retention limit and validator.
pub struct RecentScopeStore<'a> {
    db: &'a mut AppDb,
    cap: i64,
    validate_body: &'a dyn Fn(&str) -> bool,
}

impl<'a> RecentScopeStore<'a> {
    /// Binds the limit and owner-approved body validator without choosing defaults.
    pub fn new(
        db: &'a mut AppDb,
        cap: NonZeroUsize,
        validate_body: &'a dyn Fn(&str) -> bool,
    ) -> Result<Self, RecentError> {
        let cap = i64::try_from(cap.get()).map_err(|_| RecentError::Invalid)?;
        if cap == i64::MAX {
            return Err(RecentError::Invalid);
        }
        Ok(Self {
            db,
            cap,
            validate_body,
        })
    }

    /// Atomically inserts or refreshes one fingerprint and evicts oldest rows.
    pub fn put(&mut self, item: &RecentWrite<'_>) -> Result<(), RecentError> {
        if !valid_id(&item.id) || item.include_count == 0 || !(self.validate_body)(item.body) {
            return Err(RecentError::Invalid);
        }
        let tx = self
            .db
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map)?;
        tx.execute("INSERT INTO user_env_recent_scope(recent_scope_id,scope_fingerprint,fingerprint_format,scope_body,include_count,last_used_at_ms,created_at_ms,updated_at_ms,row_version) VALUES(?1,?2,1,?3,?4,?5,?5,?5,1) ON CONFLICT(scope_fingerprint,fingerprint_format) DO UPDATE SET last_used_at_ms=excluded.last_used_at_ms,updated_at_ms=excluded.updated_at_ms,row_version=row_version+1", params![item.id.as_slice(), item.fingerprint.as_slice(), item.body, item.include_count, item.now_ms]).map_err(map)?;
        tx.execute("DELETE FROM user_env_recent_scope WHERE recent_scope_id IN (SELECT recent_scope_id FROM user_env_recent_scope ORDER BY last_used_at_ms DESC,recent_scope_id ASC LIMIT -1 OFFSET ?1)", [self.cap]).map_err(map)?;
        tx.commit().map_err(map)
    }

    /// Reads summaries only; corrupt rows and excess rows fail the whole read.
    pub fn list(&self) -> Result<Vec<RecentSummary>, RecentError> {
        let mut statement = self.db.connection.prepare("SELECT recent_scope_id,include_count,last_used_at_ms FROM user_env_recent_scope ORDER BY last_used_at_ms DESC,recent_scope_id ASC LIMIT ?1").map_err(map)?;
        let rows = statement
            .query_map([self.cap + 1], read_summary)
            .map_err(map)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(map)?.validate()?);
            if result.len() > self.cap as usize {
                return Err(RecentError::Corrupt);
            }
        }
        Ok(result)
    }

    /// Reads one row and validates its body before returning it.
    pub fn get(&self, id: &[u8; 16]) -> Result<Option<RecentItem>, RecentError> {
        if !valid_id(id) {
            return Err(RecentError::Invalid);
        }
        let raw = self.db.connection.query_row("SELECT recent_scope_id,include_count,last_used_at_ms,scope_fingerprint,fingerprint_format,scope_body FROM user_env_recent_scope WHERE recent_scope_id=?1", [id.as_slice()], |row| Ok((read_summary(row)?, row.get::<_, Vec<u8>>(3)?, row.get::<_, i64>(4)?, row.get::<_, String>(5)?))).optional().map_err(map)?;
        raw.map(|(summary, fingerprint, format, body)| {
            let fingerprint = fingerprint.try_into().map_err(|_| RecentError::Corrupt)?;
            if format != 1 || !(self.validate_body)(&body) {
                return Err(RecentError::Corrupt);
            }
            Ok(RecentItem {
                summary: summary.validate()?,
                fingerprint,
                body,
            })
        })
        .transpose()
    }
}

struct RawSummary {
    id: Vec<u8>,
    count: i64,
    last: i64,
}
fn read_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawSummary> {
    Ok(RawSummary {
        id: row.get(0)?,
        count: row.get(1)?,
        last: row.get(2)?,
    })
}
impl RawSummary {
    fn validate(self) -> Result<RecentSummary, RecentError> {
        let id = self.id.try_into().map_err(|_| RecentError::Corrupt)?;
        let include_count = u32::try_from(self.count)
            .ok()
            .filter(|n| *n > 0)
            .ok_or(RecentError::Corrupt)?;
        if !valid_id(&id) {
            return Err(RecentError::Corrupt);
        }
        Ok(RecentSummary {
            id,
            include_count,
            last_used_at_ms: self.last,
        })
    }
}
fn valid_id(id: &[u8; 16]) -> bool {
    let id = uuid::Uuid::from_bytes(*id);
    id.get_version_num() == 7 && id.get_variant() == uuid::Variant::RFC4122
}
fn map(error: rusqlite::Error) -> RecentError {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => RecentError::Busy,
        Some(ErrorCode::DiskFull) => RecentError::DiskFull,
        _ => RecentError::Storage,
    }
}

#[cfg(test)]
mod tests;
