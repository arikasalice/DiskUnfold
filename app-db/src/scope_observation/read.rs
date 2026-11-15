//! Reads observations through a read-only or writer connection.

use du_core_contracts::scope_observation_log::{
    SCOPE_OBSERVATION_LIST_LIMIT, ScopeObservationList, ScopeObservationRead,
    ScopeObservationReadError,
};
use rusqlite::{ErrorCode, params};

use super::{
    SCOPE_OBSERVATION_SCHEMA_VERSION,
    decode::{LIST_SQL, decode},
};
use crate::AppDb;

impl ScopeObservationRead for AppDb {
    fn list(
        &self,
        scope_fingerprint: &[u8; 32],
        fingerprint_format: u32,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<ScopeObservationList, ScopeObservationReadError> {
        if limit == 0 || limit > SCOPE_OBSERVATION_LIST_LIMIT || fingerprint_format == 0 {
            return Err(ScopeObservationReadError::InvalidQuery);
        }
        let version: u32 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(read_error)?;
        if version < SCOPE_OBSERVATION_SCHEMA_VERSION {
            return Err(ScopeObservationReadError::UnsupportedSchema);
        }
        // One extra row tells the caller whether the limit cut the answer short.
        let fetch = i64::try_from(limit).map_err(|_| ScopeObservationReadError::InvalidQuery)? + 1;
        let mut statement = self
            .connection
            .prepare_cached(LIST_SQL)
            .map_err(read_error)?;
        let mut rows = statement
            .query(params![
                scope_fingerprint.as_slice(),
                i64::from(fingerprint_format),
                from_ms.unwrap_or(i64::MIN),
                to_ms.unwrap_or(i64::MAX),
                fetch
            ])
            .map_err(read_error)?;
        let mut listed = Vec::new();
        while let Some(stored) = rows.next().map_err(read_error)? {
            let decoded = decode(stored).map_err(read_error)?;
            listed.push(decoded.ok_or(ScopeObservationReadError::Corrupt)?);
        }
        let truncated = listed.len() > limit;
        listed.truncate(limit);
        Ok(ScopeObservationList {
            rows: listed,
            truncated,
        })
    }
}

fn read_error(error: rusqlite::Error) -> ScopeObservationReadError {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
            ScopeObservationReadError::Busy
        }
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
            ScopeObservationReadError::Corrupt
        }
        _ => match error {
            rusqlite::Error::InvalidColumnType(..)
            | rusqlite::Error::IntegralValueOutOfRange(..)
            | rusqlite::Error::Utf8Error(..) => ScopeObservationReadError::Corrupt,
            _ => ScopeObservationReadError::Storage,
        },
    }
}
