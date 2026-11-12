use std::collections::BTreeMap;

use du_core_contracts::settings::{
    SETTINGS_SCHEMA_VERSION, SettingsChanges, SettingsCommit, SettingsSnapshot, SettingsSource,
    SettingsStore, SettingsStoreError,
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

impl AppDb {
    /// Applies all registered additive-only transitions to the current schema.
    ///
    /// Every transition is validated before one final version update. Empty
    /// descriptor stages still require a registration in the ordered list.
    /// Returns the actual starting schema when a migration commits. The caller
    /// must hold the GUI startup lock and supply the complete ordered registry.
    pub fn advance_empty_settings_schema(
        &mut self,
        transitions: &[(u32, u32)],
    ) -> Result<Option<u32>, SettingsStoreError> {
        validate_empty_transitions(transitions)?;
        let target = SETTINGS_SCHEMA_VERSION;
        let observed: u32 = self.connection.query_row("SELECT schema_version FROM user_env_setting_version WHERE version_key = 'settings'", [], |row| row.get(0)).map_err(store_error)?;
        if observed >= target {
            return Ok(None);
        }
        let path = transitions_from(transitions, observed)?;
        apply_empty_settings_schema_transitions(&mut self.connection, observed, &path)?;
        Ok(Some(observed))
    }
    /// Reads the category revision, schema and raw rows from one database snapshot.
    /// Raw values allow the policy layer to isolate a malformed registered row.
    pub fn load_settings_for_restore(
        &mut self,
    ) -> Result<(u64, u32, BTreeMap<String, String>), SettingsStoreError> {
        let tx = self.connection.transaction().map_err(store_error)?;
        let (revision, schema): (i64, u32) = tx.query_row("SELECT settings_version, schema_version FROM user_env_setting_version WHERE version_key = 'settings'", [], |row| Ok((row.get(0)?, row.get(1)?))).map_err(store_error)?;
        let rows = if schema != SETTINGS_SCHEMA_VERSION {
            BTreeMap::new()
        } else {
            let mut statement = tx
                .prepare(
                    "SELECT setting_key, value_json FROM user_env_setting ORDER BY setting_key",
                )
                .map_err(store_error)?;
            let mapped = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(store_error)?;
            mapped
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map_err(store_error)?
        };
        tx.commit().map_err(store_error)?;
        Ok((
            u64::try_from(revision).map_err(|_| SettingsStoreError::Storage)?,
            schema,
            rows,
        ))
    }

    /// Moves the supplied invalid settings into quarantine in one transaction.
    ///
    /// Both the settings revision and every observed value must still match;
    /// otherwise no rows are moved. Unspecified settings are left untouched.
    pub fn quarantine_settings(
        &mut self,
        expected: u64,
        invalid: &BTreeMap<String, String>,
        timestamp_ms: i64,
    ) -> Result<u64, SettingsStoreError> {
        if invalid.is_empty() {
            return Ok(expected);
        }
        let expected = i64::try_from(expected).map_err(|_| SettingsStoreError::Conflict)?;
        let next = expected.checked_add(1).ok_or(SettingsStoreError::Storage)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let (revision, schema): (i64, u32) = tx.query_row(
            "SELECT settings_version, schema_version FROM user_env_setting_version WHERE version_key = 'settings'", [], |row| Ok((row.get(0)?, row.get(1)?))
        ).map_err(store_error)?;
        if revision != expected || schema != SETTINGS_SCHEMA_VERSION {
            return Err(SettingsStoreError::Conflict);
        }
        for (key, expected_value) in invalid {
            let raw: String = tx
                .query_row(
                    "SELECT value_json FROM user_env_setting WHERE setting_key = ?1",
                    [key],
                    |row| row.get(0),
                )
                .map_err(|error| match error {
                    Error::QueryReturnedNoRows => SettingsStoreError::Conflict,
                    other => store_error(other),
                })?;
            if &raw != expected_value {
                return Err(SettingsStoreError::Conflict);
            }
            let id = uuid::Uuid::now_v7();
            tx.execute(
                "INSERT INTO user_env_setting_quarantine
                 (quarantine_id, setting_key, raw_value_json, reason_key, quarantined_at_ms)
                 VALUES (?1, ?2, ?3, 'settings.value_invalid', ?4)",
                params![id.as_bytes().as_slice(), key, raw, timestamp_ms],
            )
            .map_err(store_error)?;
            tx.execute("DELETE FROM user_env_setting WHERE setting_key = ?1", [key])
                .map_err(store_error)?;
        }
        tx.execute("UPDATE user_env_setting_version SET settings_version = ?1, updated_at_ms = ?2 WHERE version_key = 'settings'", params![next, timestamp_ms]).map_err(store_error)?;
        tx.commit().map_err(store_error)?;
        u64::try_from(next).map_err(|_| SettingsStoreError::Storage)
    }
}

fn apply_empty_settings_schema_transitions(
    connection: &mut rusqlite::Connection,
    observed: u32,
    path: &[(u32, u32)],
) -> Result<(), SettingsStoreError> {
    let target = path
        .last()
        .map(|(_, to)| *to)
        .filter(|target| *target == SETTINGS_SCHEMA_VERSION)
        .ok_or(SettingsStoreError::Conflict)?;
    if path.first().map(|(from, _)| *from) != Some(observed) {
        return Err(SettingsStoreError::Conflict);
    }
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(store_error)?;
    let schema: u32 = tx
        .query_row(
            "SELECT schema_version FROM user_env_setting_version WHERE version_key = 'settings'",
            [],
            |row| row.get(0),
        )
        .map_err(store_error)?;
    if schema != observed {
        return Err(SettingsStoreError::Conflict);
    }
    let changed = tx.execute(
        "UPDATE user_env_setting_version SET schema_version = ?1 WHERE version_key = 'settings' AND schema_version = ?2",
        params![target, observed],
    ).map_err(store_error)?;
    let actual: u32 = tx
        .query_row(
            "SELECT schema_version FROM user_env_setting_version WHERE version_key = 'settings'",
            [],
            |row| row.get(0),
        )
        .map_err(store_error)?;
    if changed != 1 || actual != target {
        return Err(SettingsStoreError::Conflict);
    }
    tx.commit().map_err(store_error)
}

fn validate_empty_transitions(transitions: &[(u32, u32)]) -> Result<(), SettingsStoreError> {
    let mut expected = 1;
    for &(from, to) in transitions {
        if from != expected || to != from.checked_add(1).ok_or(SettingsStoreError::Conflict)? {
            return Err(SettingsStoreError::Conflict);
        }
        expected = to;
    }
    if transitions.is_empty() || expected != SETTINGS_SCHEMA_VERSION {
        return Err(SettingsStoreError::Conflict);
    }
    Ok(())
}

fn transitions_from(
    transitions: &[(u32, u32)],
    schema: u32,
) -> Result<Vec<(u32, u32)>, SettingsStoreError> {
    let mut current = schema;
    let mut path = Vec::new();
    for &(from, to) in transitions {
        if to <= current {
            continue;
        }
        if from != current {
            return Err(SettingsStoreError::Conflict);
        }
        path.push((from, to));
        current = to;
    }
    if current != SETTINGS_SCHEMA_VERSION {
        return Err(SettingsStoreError::Conflict);
    }
    Ok(path)
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
                "UPDATE user_env_setting_version SET settings_version = ?1, updated_at_ms = ?2 WHERE version_key = 'settings' AND settings_version = ?3 AND schema_version = ?4",
                params![next, context.timestamp_ms, expected, SETTINGS_SCHEMA_VERSION],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_mismatch_at_transaction_start_preserves_newer_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let mut db = AppDb::create_gui(&dir.path().join("schema-race.db"))
            .map_err(|error| format!("{error:?}"))?;
        db.connection.execute(
            "UPDATE user_env_setting_version SET schema_version=?1,settings_version=9,updated_at_ms=11",
            [SETTINGS_SCHEMA_VERSION],
        )?;
        db.connection.execute_batch(
            "INSERT INTO user_env_setting VALUES ('scope.remember_recent','false','screen',3,7,4);",
        )?;
        let before: (u32, i64, i64, String, String, i64, i64, i64) = db.connection.query_row(
            "SELECT v.schema_version, v.settings_version, v.updated_at_ms, s.setting_key, s.value_json, s.created_at_ms, s.updated_at_ms, s.row_version FROM user_env_setting_version v CROSS JOIN user_env_setting s",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
        )?;
        assert_eq!(
            apply_empty_settings_schema_transitions(
                &mut db.connection,
                1,
                &[(1, 2), (2, SETTINGS_SCHEMA_VERSION)],
            ),
            Err(SettingsStoreError::Conflict)
        );
        let after: (u32, i64, i64, String, String, i64, i64, i64) = db.connection.query_row(
            "SELECT v.schema_version, v.settings_version, v.updated_at_ms, s.setting_key, s.value_json, s.created_at_ms, s.updated_at_ms, s.row_version FROM user_env_setting_version v CROSS JOIN user_env_setting s",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
        )?;
        assert_eq!(after, before);
        Ok(())
    }
}
