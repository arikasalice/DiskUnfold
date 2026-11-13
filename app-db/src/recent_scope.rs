//! SQLite mechanics for F01 recent scopes.
//!
//! The owner must bind a validated body and fingerprint to an admitted GUI
//! scan, consult the current setting, and supply a published retention limit.
//! This module neither admits scans nor determines the scope body format.

use std::num::NonZeroUsize;

use du_core_contracts::settings::SETTINGS_SCHEMA_VERSION;
use rusqlite::{ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params};

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

/// Distinguishes failure to acquire the final write transaction from errors
/// after its setting read has started.
#[derive(Debug, PartialEq, Eq)]
pub enum RecentWriteError {
    /// SQLite was busy before the checked setting read began.
    BeginBusy,
    /// A validation, setting read, write, or commit failed after begin.
    Other(RecentError),
}

impl RecentWriteError {
    /// Maps the staged result back to the legacy storage error classification.
    fn into_recent_error(self) -> RecentError {
        match self {
            Self::BeginBusy => RecentError::Busy,
            Self::Other(error) => error,
        }
    }
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

/// A selected row with validated metadata. The owner validates its body.
#[derive(Debug, PartialEq, Eq)]
pub struct RecentItem {
    /// List summary.
    pub summary: RecentSummary,
    /// Persisted scope fingerprint.
    pub fingerprint: [u8; 32],
    /// Serialized scope body.
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
        let cap = sql_cap(cap)?;
        Ok(Self {
            db,
            cap,
            validate_body,
        })
    }

    /// Atomically inserts or refreshes one fingerprint and evicts oldest rows.
    /// A delayed write with an older use time cannot replace a newer use.
    pub fn put(&mut self, item: &RecentWrite<'_>) -> Result<(), RecentError> {
        self.put_if(item, |_, cap| Ok(Some(cap))).map(|_| ())
    }

    /// Evaluates a caller-owned setting policy in the same transaction as the write.
    /// Missing, invalid or unreadable settings are supplied as `None`. Failures to
    /// begin, write or commit the transaction are returned. The callback must not
    /// access this database or do I/O.
    pub fn put_with_setting(
        &mut self,
        item: &RecentWrite<'_>,
        key: &str,
        allowed: impl FnOnce(Option<bool>) -> bool,
    ) -> Result<bool, RecentError> {
        self.put_if(item, |tx, cap| {
            Ok(allowed(read_setting_bool(tx, key)).then_some(cap))
        })
    }

    /// Evaluates a setting in the same transaction as the write, propagating
    /// database read failures while retaining safe missing-table recovery.
    pub fn put_with_setting_checked(
        &mut self,
        item: &RecentWrite<'_>,
        key: &str,
        allowed: impl FnOnce(Option<bool>) -> bool,
    ) -> Result<bool, RecentError> {
        self.put_with_setting_checked_staged(item, key, allowed)
            .map_err(RecentWriteError::into_recent_error)
    }

    /// Like [`Self::put_with_setting_checked`], preserving the transaction stage
    /// when the immediate transaction cannot be acquired.
    pub fn put_with_setting_checked_staged(
        &mut self,
        item: &RecentWrite<'_>,
        key: &str,
        allowed: impl FnOnce(Option<bool>) -> bool,
    ) -> Result<bool, RecentWriteError> {
        self.put_if_checked(item, |tx, cap| {
            Ok(allowed(read_setting_bool_checked(tx, key)?).then_some(cap))
        })
    }

    /// Resolves a setting in its own deferred read transaction without writing a row.
    ///
    /// Use this before expensive, read-only preparation that may fail before a
    /// `RecentWrite` can be built. A later write must still use
    /// `put_with_setting_checked_staged`, which rechecks the value atomically with
    /// insertion and eviction. The resolver must not perform I/O or access this
    /// database.
    pub fn resolve_with_setting<T>(
        &mut self,
        key: &str,
        resolve: impl FnOnce(Option<bool>) -> T,
    ) -> Result<T, RecentError> {
        let tx = self
            .db
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(map)?;
        let value = read_setting_bool_checked(&tx, key)?;
        let resolved = resolve(value);
        tx.commit().map_err(map)?;
        Ok(resolved)
    }

    /// Reads the remember and retention settings together before a settings-driven write.
    ///
    /// FR8.HD1.app-host.DD2 and FR14.HD1.app-host.DD1: settings are read under
    /// the same immediate transaction as insertion, refresh, and eviction.
    /// Missing or malformed setting values reach `resolve` as `None`. A missing
    /// settings table also reaches `resolve` as `None` only when the error is a
    /// known missing-table error and the active transaction passes a read probe.
    /// Other SQL failures abort the transaction. `None` from `resolve` skips
    /// the write.
    pub fn put_with_settings(
        &mut self,
        item: &RecentWrite<'_>,
        remember_key: &str,
        limit_key: &str,
        resolve: impl FnOnce(Option<&str>, Option<&str>) -> Option<NonZeroUsize>,
    ) -> Result<bool, RecentError> {
        self.put_if(item, |tx, _static_cap| {
            let stored: Option<(
                rusqlite::types::Value,
                Option<rusqlite::types::Value>,
                Option<rusqlite::types::Value>,
            )> = match tx
                .query_row(
                    "SELECT v.schema_version, remember.value_json, retention.value_json
                 FROM user_env_setting_version v
                 LEFT JOIN user_env_setting remember ON remember.setting_key = ?1
                 LEFT JOIN user_env_setting retention ON retention.setting_key = ?2
                 WHERE v.version_key = 'settings'",
                    [remember_key, limit_key],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
            {
                Ok(stored) => stored,
                Err(error) if settings_table_unavailable(tx, &error)? => None,
                Err(error) => return Err(map(error)),
            };
            let (remember, limit) = stored
                .filter(|(schema, _, _)| {
                    matches!(
                        schema,
                        rusqlite::types::Value::Integer(version)
                            if *version == i64::from(SETTINGS_SCHEMA_VERSION)
                    )
                })
                .map_or((None, None), |(_, remember, limit)| {
                    (setting_text(remember), setting_text(limit))
                });
            resolve(remember.as_deref(), limit.as_deref())
                .map(sql_cap)
                .transpose()
        })
    }

    fn put_if(
        &mut self,
        item: &RecentWrite<'_>,
        allowed: impl FnOnce(&Transaction<'_>, i64) -> Result<Option<i64>, RecentError>,
    ) -> Result<bool, RecentError> {
        self.put_if_checked(item, allowed)
            .map_err(RecentWriteError::into_recent_error)
    }

    fn put_if_checked(
        &mut self,
        item: &RecentWrite<'_>,
        allowed: impl FnOnce(&Transaction<'_>, i64) -> Result<Option<i64>, RecentError>,
    ) -> Result<bool, RecentWriteError> {
        if !valid_id(&item.id) || item.include_count == 0 || !(self.validate_body)(item.body) {
            return Err(RecentWriteError::Other(RecentError::Invalid));
        }
        let tx = self
            .db
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_begin)?;
        let Some(cap) = allowed(&tx, self.cap).map_err(RecentWriteError::Other)? else {
            return Ok(false);
        };
        tx.execute(
            "INSERT INTO user_env_recent_scope(
                 recent_scope_id,scope_fingerprint,fingerprint_format,scope_body,include_count,
                 last_used_at_ms,created_at_ms,updated_at_ms,row_version
             ) VALUES(?1,?2,1,?3,?4,?5,?5,?5,1)
             ON CONFLICT(scope_fingerprint,fingerprint_format) DO UPDATE SET
                 last_used_at_ms=excluded.last_used_at_ms,
                 updated_at_ms=excluded.updated_at_ms,
                 row_version=row_version+1
             WHERE excluded.last_used_at_ms >= user_env_recent_scope.last_used_at_ms",
            params![
                item.id.as_slice(),
                item.fingerprint.as_slice(),
                item.body,
                item.include_count,
                item.now_ms
            ],
        )
        .map_err(map)
        .map_err(RecentWriteError::Other)?;
        tx.execute("DELETE FROM user_env_recent_scope WHERE recent_scope_id IN (SELECT recent_scope_id FROM user_env_recent_scope ORDER BY last_used_at_ms DESC,recent_scope_id ASC LIMIT -1 OFFSET ?1)", [cap])
            .map_err(map)
            .map_err(RecentWriteError::Other)?;
        tx.commit().map_err(map).map_err(RecentWriteError::Other)?;
        Ok(true)
    }

    /// Reads summaries only; corrupt rows and excess rows fail the whole read.
    pub fn list(&self) -> Result<Vec<RecentSummary>, RecentError> {
        read_summaries(self.db, self.cap)
    }

    /// Reads one row and validates its body before returning it.
    pub fn get(&self, id: &[u8; 16]) -> Result<Option<RecentItem>, RecentError> {
        let row = read_recent_scope(self.db, id)?;
        if row
            .as_ref()
            .is_some_and(|row| !(self.validate_body)(&row.body))
        {
            return Err(RecentError::Corrupt);
        }
        Ok(row)
    }
}

fn read_setting_bool(tx: &Transaction<'_>, key: &str) -> Option<bool> {
    let stored: Option<(i64, Option<String>)> = tx
        .query_row(
            "SELECT v.schema_version, s.value_json
             FROM user_env_setting_version v
             LEFT JOIN user_env_setting s ON s.setting_key = ?1
             WHERE v.version_key = 'settings'",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .ok()
        .flatten();
    stored.and_then(|(schema, value)| {
        if schema != i64::from(SETTINGS_SCHEMA_VERSION) {
            return None;
        }
        serde_json::from_str::<bool>(value.as_deref()?).ok()
    })
}

fn read_setting_bool_checked(tx: &Transaction<'_>, key: &str) -> Result<Option<bool>, RecentError> {
    let stored: Option<(rusqlite::types::Value, Option<rusqlite::types::Value>)> = match tx
        .query_row(
            "SELECT v.schema_version, s.value_json
                 FROM user_env_setting_version v
                 LEFT JOIN user_env_setting s ON s.setting_key = ?1
                 WHERE v.version_key = 'settings'",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
    {
        Ok(stored) => stored,
        Err(error) if settings_table_unavailable(tx, &error)? => None,
        Err(error) => return Err(map(error)),
    };
    let Some((schema, value)) = stored else {
        return Ok(None);
    };
    if !matches!(schema, rusqlite::types::Value::Integer(version) if version == i64::from(SETTINGS_SCHEMA_VERSION))
    {
        return Ok(None);
    }
    let value = match value {
        Some(rusqlite::types::Value::Text(value)) => value,
        Some(_) | None => return Ok(None),
    };
    Ok(serde_json::from_str::<bool>(&value).ok())
}

/// Reads one row's metadata and serialized body for owner validation.
pub fn read_recent_scope(db: &AppDb, id: &[u8; 16]) -> Result<Option<RecentItem>, RecentError> {
    if !valid_id(id) {
        return Err(RecentError::Invalid);
    }
    let raw = db.connection.query_row("SELECT recent_scope_id,include_count,last_used_at_ms,scope_fingerprint,fingerprint_format,scope_body FROM user_env_recent_scope WHERE recent_scope_id=?1", [id.as_slice()], |row| Ok((read_summary(row)?, row.get::<_, Vec<u8>>(3)?, row.get::<_, i64>(4)?, row.get::<_, String>(5)?))).optional().map_err(map)?;
    raw.map(|(summary, fingerprint, format, body)| {
        if format != 1 {
            return Err(RecentError::Corrupt);
        }
        Ok(RecentItem {
            summary: summary.validate()?,
            fingerprint: fingerprint.try_into().map_err(|_| RecentError::Corrupt)?,
            body,
        })
    })
    .transpose()
}

/// Reads the bounded summary projection without decoding stored bodies.
pub fn list_recent_scopes(
    db: &AppDb,
    cap: NonZeroUsize,
) -> Result<Vec<RecentSummary>, RecentError> {
    read_summaries(db, sql_cap(cap)?)
}

fn sql_cap(cap: NonZeroUsize) -> Result<i64, RecentError> {
    let cap = i64::try_from(cap.get()).map_err(|_| RecentError::Invalid)?;
    if cap == i64::MAX {
        return Err(RecentError::Invalid);
    }
    Ok(cap)
}

fn setting_text(value: Option<rusqlite::types::Value>) -> Option<String> {
    match value {
        Some(rusqlite::types::Value::Text(value)) => Some(value),
        Some(_) | None => None,
    }
}

fn settings_table_unavailable(
    tx: &Transaction<'_>,
    error: &rusqlite::Error,
) -> Result<bool, RecentError> {
    let expected_missing_table = matches!(
        error,
        rusqlite::Error::SqliteFailure(code, Some(message))
            if code.code == ErrorCode::Unknown
                && matches!(
                    message.as_str(),
                    "no such table: user_env_setting"
                        | "no such table: user_env_setting_version"
                )
    );
    if !expected_missing_table || tx.is_autocommit() {
        return Ok(false);
    }

    let probe: i64 = tx
        .query_row("SELECT 1", [], |row| row.get(0))
        .map_err(map)?;
    Ok(probe == 1 && !tx.is_autocommit())
}

fn read_summaries(db: &AppDb, cap: i64) -> Result<Vec<RecentSummary>, RecentError> {
    let mut statement = db.connection.prepare("SELECT recent_scope_id,include_count,last_used_at_ms FROM user_env_recent_scope ORDER BY last_used_at_ms DESC,recent_scope_id ASC LIMIT ?1").map_err(map)?;
    let rows = statement.query_map([cap + 1], read_summary).map_err(map)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(map)?.validate()?);
        if result.len() > cap as usize {
            return Err(RecentError::Corrupt);
        }
    }
    Ok(result)
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
fn map_begin(error: rusqlite::Error) -> RecentWriteError {
    match map(error) {
        RecentError::Busy => RecentWriteError::BeginBusy,
        other => RecentWriteError::Other(other),
    }
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
