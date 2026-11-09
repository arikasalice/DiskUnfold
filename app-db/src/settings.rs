use std::collections::BTreeMap;

use du_core_contracts::settings::{
    SettingsChanges, SettingsCommit, SettingsSnapshot, SettingsSource, SettingsStore,
    SettingsStoreError,
};
use rusqlite::{Error, ErrorCode, TransactionBehavior, params};

use crate::AppDb;

// Natural-key exceptions: setting_key and the single version_key.
pub(crate) const SCHEMA: &str = "
CREATE TABLE user_env_setting_version (
 version_key TEXT PRIMARY KEY CHECK(version_key = 'settings'),
 settings_version INTEGER NOT NULL CHECK(settings_version >= 0),
 schema_version INTEGER NOT NULL CHECK(schema_version >= 1),
 updated_at_ms INTEGER NOT NULL
) STRICT;
INSERT INTO user_env_setting_version VALUES ('settings', 0, 1, 0);
CREATE TABLE user_env_setting (
 setting_key TEXT PRIMARY KEY,
 value_json TEXT NOT NULL CHECK(json_valid(value_json)),
 modified_source TEXT NOT NULL CHECK(modified_source IN ('screen','import','reset','owner','migration')),
 created_at_ms INTEGER NOT NULL,
 updated_at_ms INTEGER NOT NULL,
 row_version INTEGER NOT NULL CHECK(row_version >= 1)
) STRICT;
CREATE TABLE user_env_setting_quarantine (
 quarantine_id BLOB PRIMARY KEY CHECK(length(quarantine_id) = 16),
 setting_key TEXT NOT NULL,
 raw_value_json TEXT NOT NULL,
 reason_key TEXT NOT NULL,
 quarantined_at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX ix_user_env_setting_quarantine_key ON user_env_setting_quarantine(setting_key);
";

fn source_name(source: SettingsSource) -> &'static str {
    match source {
        SettingsSource::Screen => "screen",
        SettingsSource::Import => "import",
        SettingsSource::Reset => "reset",
        SettingsSource::Owner => "owner",
        SettingsSource::Migration => "migration",
    }
}

fn store_error(error: Error) -> SettingsStoreError {
    match error {
        Error::SqliteFailure(ref failure, _)
            if matches!(
                failure.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            SettingsStoreError::Busy
        }
        _ => SettingsStoreError::Storage,
    }
}

impl SettingsStore for AppDb {
    fn load(&mut self) -> Result<SettingsSnapshot, SettingsStoreError> {
        let tx = self.connection.transaction().map_err(store_error)?;
        let (revision, schema_version): (i64, u32) = tx
            .query_row(
                "SELECT settings_version, schema_version FROM user_env_setting_version WHERE version_key = 'settings'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(store_error)?;
        let revision = u64::try_from(revision).map_err(|_| SettingsStoreError::Storage)?;
        let values = {
            let mut statement = tx
                .prepare(
                    "SELECT setting_key, value_json FROM user_env_setting ORDER BY setting_key",
                )
                .map_err(store_error)?;
            let rows = statement
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(store_error)?;
            let mut values = BTreeMap::new();
            for row in rows {
                let (key, value) = row.map_err(store_error)?;
                values.insert(
                    key,
                    serde_json::from_str(&value).map_err(|_| SettingsStoreError::Storage)?,
                );
            }
            values
        };
        tx.commit().map_err(store_error)?;
        Ok(SettingsSnapshot {
            revision,
            schema_version,
            values,
        })
    }

    fn commit(
        &mut self,
        expected: u64,
        changes: &SettingsChanges,
        context: SettingsCommit,
    ) -> Result<u64, SettingsStoreError> {
        let expected = i64::try_from(expected).map_err(|_| SettingsStoreError::Conflict)?;
        let next = expected.checked_add(1).ok_or(SettingsStoreError::Storage)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        if tx
            .execute(
                "UPDATE user_env_setting_version SET settings_version = ?1, updated_at_ms = ?2 WHERE version_key = 'settings' AND settings_version = ?3",
                params![next, context.timestamp_ms, expected],
            )
            .map_err(store_error)?
            != 1
        {
            return Err(SettingsStoreError::Conflict);
        }
        for (key, value) in changes {
            match value {
                Some(value) => {
                    let encoded =
                        serde_json::to_string(value).map_err(|_| SettingsStoreError::Storage)?;
                    tx.execute("INSERT INTO user_env_setting (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version) VALUES (?1, ?2, ?3, ?4, ?4, 1) ON CONFLICT(setting_key) DO UPDATE SET value_json = excluded.value_json, modified_source = excluded.modified_source, updated_at_ms = excluded.updated_at_ms, row_version = user_env_setting.row_version + 1", params![key, encoded, source_name(context.source), context.timestamp_ms]).map_err(store_error)?;
                }
                None => {
                    tx.execute("DELETE FROM user_env_setting WHERE setting_key = ?1", [key])
                        .map_err(store_error)?;
                }
            }
        }
        tx.commit().map_err(store_error)?;
        u64::try_from(next).map_err(|_| SettingsStoreError::Storage)
    }
}
