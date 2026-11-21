//! SQLite storage for the F07 snapshot index.

use std::collections::HashMap;

use du_core_contracts::{
    Id,
    snapshot_index::{
        SnapshotIndexCompleteness, SnapshotIndexCursor, SnapshotIndexEntry, SnapshotIndexError,
        SnapshotIndexPage, SnapshotIndexPort, SnapshotIndexRegisterResult, SnapshotIndexRow,
        SnapshotIndexSourceKind, SnapshotIndexState, SnapshotIndexVolume,
        SnapshotVolumeIdentityCapability, SnapshotVolumeIdentityKind,
    },
};
use rusqlite::{
    ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params, params_from_iter,
};

use crate::AppDb;

/// Schema version that introduced the snapshot index.
pub const SNAPSHOT_INDEX_SCHEMA_VERSION: u32 = 11;

const LIST_FIELDS: &str = "snapshot_id, file_name, source_version_id, lineage_id, source_kind,
    observed_at_ms, created_at_ms, scope_label, condition_summary, completeness,
    format_version, body_size, body_hash, state, verify_failed_at_ms, updated_at_ms, row_version";

impl SnapshotIndexPort for AppDb {
    fn register(
        &mut self,
        row: &SnapshotIndexRow,
        volumes: &[SnapshotIndexVolume],
        baseline_pinned_at_ms: Option<i64>,
    ) -> Result<SnapshotIndexRegisterResult, SnapshotIndexError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(write_error)?;
        ensure_schema(&tx)?;

        if let Some(existing) = active_by_source(&tx, &row.source_version_id)? {
            let list_seq = read_list_seq(&tx)?;
            drop(tx);
            return Ok(SnapshotIndexRegisterResult {
                snapshot_id: existing,
                list_seq,
                inserted: false,
            });
        }

        if row.state != SnapshotIndexState::Active
            || row.format_version == 0
            || !row.scope_label.is_empty() && row.scope_label.len() != 32
            || volumes.iter().any(|volume| {
                volume.volume_index == u16::MAX
                    || volume.identity_kind != SnapshotVolumeIdentityKind::None
                    || volume.identity_capability != SnapshotVolumeIdentityCapability::Unknown
            })
        {
            return Err(SnapshotIndexError::InvalidInput);
        }

        let snapshot_id = required_id_bytes(&row.snapshot_id)?;
        let source_version_id = required_id_bytes(&row.source_version_id)?;
        let lineage_id = required_id_bytes(&row.lineage_id)?;
        tx.execute(
            "INSERT INTO snapshot_store_snapshot (
                 snapshot_id, file_name, source_version_id, lineage_id, source_kind,
                 observed_at_ms, created_at_ms, scope_label, condition_summary, completeness,
                 format_version, body_size, body_hash, state, verify_failed_at_ms,
                 updated_at_ms, row_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'active',
                     ?14, ?15, ?16)",
            params![
                snapshot_id.as_slice(),
                row.file_name,
                source_version_id.as_slice(),
                lineage_id.as_slice(),
                source_kind(row.source_kind),
                row.observed_at_ms,
                row.created_at_ms,
                row.scope_label,
                row.condition_summary,
                row.completeness.map(completeness),
                i64::from(row.format_version),
                row.body_size,
                row.body_hash.as_slice(),
                row.verify_failed_at_ms,
                row.updated_at_ms,
                row.row_version,
            ],
        )
        .map_err(write_error)?;

        for volume in volumes {
            tx.execute(
                "INSERT INTO snapshot_store_snapshot_volume (
                     snapshot_id, volume_index, observed_volume_id, identity_kind,
                     identity_capability)
                 VALUES (?1, ?2, ?3, 'none', 'unknown')",
                params![
                    snapshot_id.as_slice(),
                    i64::from(volume.volume_index),
                    volume.observed_volume_id,
                ],
            )
            .map_err(write_error)?;
        }
        if let Some(pinned_at_ms) = baseline_pinned_at_ms {
            tx.execute(
                "INSERT INTO snapshot_store_baseline(snapshot_id, pinned_at_ms) VALUES (?1, ?2)",
                params![snapshot_id.as_slice(), pinned_at_ms],
            )
            .map_err(write_error)?;
        }

        let list_seq = read_list_seq(&tx)?;
        let next_seq = list_seq.checked_add(1).ok_or(SnapshotIndexError::Storage)?;
        let updated = tx
            .execute(
                "UPDATE snapshot_store_list_state SET list_seq = ?1
                 WHERE name = 'snapshot_list' AND list_seq = ?2",
                params![next_seq, list_seq],
            )
            .map_err(write_error)?;
        if updated != 1 {
            return Err(SnapshotIndexError::Corrupt);
        }
        tx.commit().map_err(write_error)?;
        Ok(SnapshotIndexRegisterResult {
            snapshot_id: row.snapshot_id.clone(),
            list_seq: next_seq,
            inserted: true,
        })
    }

    fn find_active_by_source(
        &mut self,
        source_version_id: &Id,
    ) -> Result<Option<Id>, SnapshotIndexError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(read_error)?;
        ensure_schema(&tx)?;
        let found = active_by_source(&tx, source_version_id)?;
        tx.commit().map_err(read_error)?;
        Ok(found)
    }

    fn list_active(
        &mut self,
        limit: usize,
        after: Option<&SnapshotIndexCursor>,
    ) -> Result<SnapshotIndexPage, SnapshotIndexError> {
        if limit == 0 || limit > 1_000 {
            return Err(SnapshotIndexError::InvalidInput);
        }
        let fetch = i64::try_from(limit)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(SnapshotIndexError::InvalidInput)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(read_error)?;
        ensure_schema(&tx)?;
        let list_seq = read_list_seq(&tx)?;
        if after.is_some_and(|cursor| cursor.list_seq != list_seq) {
            return Err(SnapshotIndexError::StaleCursor);
        }
        let cursor_bytes = after
            .map(|cursor| required_id_bytes(&cursor.snapshot_id))
            .transpose()?;
        let cursor_id = cursor_bytes.unwrap_or([0; 16]);
        let mut statement = tx
            .prepare_cached(&format!(
                "SELECT {LIST_FIELDS}, EXISTS(
                     SELECT 1 FROM snapshot_store_baseline b
                     WHERE b.snapshot_id = s.snapshot_id)
                 FROM snapshot_store_snapshot s
                 WHERE s.state = 'active' AND (
                     ?1 = 0 OR
                     (?2 IS NULL AND s.observed_at_ms IS NULL AND s.snapshot_id > ?3) OR
                     (?2 IS NOT NULL AND (s.observed_at_ms IS NULL OR
                         s.observed_at_ms < ?2 OR
                         (s.observed_at_ms = ?2 AND s.snapshot_id > ?3)))
                 )
                 ORDER BY s.observed_at_ms IS NULL, s.observed_at_ms DESC, s.snapshot_id ASC
                 LIMIT ?4"
            ))
            .map_err(read_error)?;
        let mut rows = statement
            .query(params![
                if after.is_some() { 1_i64 } else { 0_i64 },
                after.and_then(|cursor| cursor.observed_at_ms),
                cursor_id.as_slice(),
                fetch,
            ])
            .map_err(read_error)?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next().map_err(read_error)? {
            let snapshot = decode_snapshot(row).map_err(read_error)?;
            let pinned: bool = row.get(17).map_err(read_error)?;
            entries.push(SnapshotIndexEntry {
                row: snapshot,
                volumes: Vec::new(),
                pinned,
            });
        }
        drop(rows);
        drop(statement);

        let has_more = entries.len() > limit;
        entries.truncate(limit);
        load_volumes(&tx, &mut entries)?;
        let next_cursor = if has_more {
            entries.last().map(|entry| SnapshotIndexCursor {
                list_seq,
                observed_at_ms: entry.row.observed_at_ms,
                snapshot_id: entry.row.snapshot_id.clone(),
            })
        } else {
            None
        };
        tx.commit().map_err(read_error)?;
        Ok(SnapshotIndexPage {
            rows: entries,
            list_seq,
            next_cursor,
        })
    }
}

fn ensure_schema(tx: &Transaction<'_>) -> Result<(), SnapshotIndexError> {
    let version: u32 = tx
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(read_error)?;
    if version < SNAPSHOT_INDEX_SCHEMA_VERSION {
        Err(SnapshotIndexError::UnsupportedSchema)
    } else {
        Ok(())
    }
}

fn active_by_source(
    tx: &Transaction<'_>,
    source_version_id: &Id,
) -> Result<Option<Id>, SnapshotIndexError> {
    let source = required_id_bytes(source_version_id)?;
    let found: Option<Vec<u8>> = tx
        .query_row(
            "SELECT snapshot_id FROM snapshot_store_snapshot
             WHERE source_version_id = ?1 AND state = 'active'",
            [source.as_slice()],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    found
        .map(|bytes| decode_id(&bytes))
        .transpose()
        .map_err(read_error)
}

fn read_list_seq(tx: &Transaction<'_>) -> Result<i64, SnapshotIndexError> {
    let seq: Option<i64> = tx
        .query_row(
            "SELECT list_seq FROM snapshot_store_list_state WHERE name = 'snapshot_list'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)?;
    seq.filter(|seq| *seq >= 0)
        .ok_or(SnapshotIndexError::Corrupt)
}

fn required_id_bytes(id: &Id) -> Result<[u8; 16], SnapshotIndexError> {
    id.v7_bytes().ok_or(SnapshotIndexError::InvalidInput)
}

fn decode_id(bytes: &[u8]) -> Result<Id, rusqlite::Error> {
    let raw: [u8; 16] = bytes
        .try_into()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let uuid = uuid::Uuid::from_bytes(raw);
    if uuid.get_version_num() != 7 || uuid.get_variant() != uuid::Variant::RFC4122 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Id::parse(&uuid.to_string()).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn decode_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotIndexRow> {
    let source_kind: String = row.get(4)?;
    let completeness: Option<String> = row.get(9)?;
    let state: String = row.get(13)?;
    let format_version: i64 = row.get(10)?;
    let body_hash: Vec<u8> = row.get(12)?;
    Ok(SnapshotIndexRow {
        snapshot_id: decode_id(&row.get::<_, Vec<u8>>(0)?)?,
        file_name: row.get(1)?,
        source_version_id: decode_id(&row.get::<_, Vec<u8>>(2)?)?,
        lineage_id: decode_id(&row.get::<_, Vec<u8>>(3)?)?,
        source_kind: match source_kind.as_str() {
            "local" => SnapshotIndexSourceKind::Local,
            "imported" => SnapshotIndexSourceKind::Imported,
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
        observed_at_ms: row.get(5)?,
        created_at_ms: row.get(6)?,
        scope_label: row.get(7)?,
        condition_summary: row.get(8)?,
        completeness: completeness
            .map(|value| match value.as_str() {
                "complete" => Ok(SnapshotIndexCompleteness::Complete),
                "partial" => Ok(SnapshotIndexCompleteness::Partial),
                "missing" => Ok(SnapshotIndexCompleteness::Missing),
                _ => Err(rusqlite::Error::InvalidQuery),
            })
            .transpose()?,
        format_version: u16::try_from(format_version)
            .ok()
            .filter(|value| *value > 0)
            .ok_or(rusqlite::Error::InvalidQuery)?,
        body_size: row.get(11)?,
        body_hash: body_hash
            .try_into()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        state: match state.as_str() {
            "active" => SnapshotIndexState::Active,
            "deleted" => SnapshotIndexState::Deleted,
            "removed" => SnapshotIndexState::Removed,
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
        verify_failed_at_ms: row.get(14)?,
        updated_at_ms: row.get(15)?,
        row_version: row.get(16)?,
    })
}

fn load_volumes(
    tx: &Transaction<'_>,
    entries: &mut [SnapshotIndexEntry],
) -> Result<(), SnapshotIndexError> {
    if entries.is_empty() {
        return Ok(());
    }
    let ids = entries
        .iter()
        .map(|entry| required_id_bytes(&entry.row.snapshot_id))
        .collect::<Result<Vec<_>, _>>()?;
    let positions: HashMap<[u8; 16], usize> = ids
        .iter()
        .copied()
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();
    let placeholders = (1..=ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT snapshot_id, volume_index, observed_volume_id, identity_kind,
                identity_capability
         FROM snapshot_store_snapshot_volume
         WHERE snapshot_id IN ({placeholders})
         ORDER BY snapshot_id, volume_index"
    );
    let mut statement = tx.prepare(&sql).map_err(read_error)?;
    let mut rows = statement
        .query(params_from_iter(ids.iter().map(|id| id.as_slice())))
        .map_err(read_error)?;
    while let Some(row) = rows.next().map_err(read_error)? {
        let id: [u8; 16] = row
            .get::<_, Vec<u8>>(0)
            .map_err(read_error)?
            .try_into()
            .map_err(|_| SnapshotIndexError::Corrupt)?;
        let position = *positions.get(&id).ok_or(SnapshotIndexError::Corrupt)?;
        let volume_index = u16::try_from(row.get::<_, i64>(1).map_err(read_error)?)
            .ok()
            .filter(|value| *value < u16::MAX)
            .ok_or(SnapshotIndexError::Corrupt)?;
        let identity_kind: String = row.get(3).map_err(read_error)?;
        let identity_capability: String = row.get(4).map_err(read_error)?;
        entries[position].volumes.push(SnapshotIndexVolume {
            volume_index,
            observed_volume_id: row.get(2).map_err(read_error)?,
            identity_kind: match identity_kind.as_str() {
                "guid" => SnapshotVolumeIdentityKind::Guid,
                "uuid" => SnapshotVolumeIdentityKind::Uuid,
                "fs_uuid" => SnapshotVolumeIdentityKind::FsUuid,
                "none" => SnapshotVolumeIdentityKind::None,
                _ => return Err(SnapshotIndexError::Corrupt),
            },
            identity_capability: match identity_capability.as_str() {
                "supported" => SnapshotVolumeIdentityCapability::Supported,
                "partial" => SnapshotVolumeIdentityCapability::Partial,
                "unsupported" => SnapshotVolumeIdentityCapability::Unsupported,
                "unknown" => SnapshotVolumeIdentityCapability::Unknown,
                _ => return Err(SnapshotIndexError::Corrupt),
            },
        });
    }
    Ok(())
}

fn source_kind(value: SnapshotIndexSourceKind) -> &'static str {
    match value {
        SnapshotIndexSourceKind::Local => "local",
        SnapshotIndexSourceKind::Imported => "imported",
    }
}

fn completeness(value: SnapshotIndexCompleteness) -> &'static str {
    match value {
        SnapshotIndexCompleteness::Complete => "complete",
        SnapshotIndexCompleteness::Partial => "partial",
        SnapshotIndexCompleteness::Missing => "missing",
    }
}

fn read_error(error: rusqlite::Error) -> SnapshotIndexError {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => SnapshotIndexError::Busy,
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => SnapshotIndexError::Corrupt,
        _ => match error {
            rusqlite::Error::InvalidColumnType(..)
            | rusqlite::Error::IntegralValueOutOfRange(..)
            | rusqlite::Error::Utf8Error(..)
            | rusqlite::Error::InvalidQuery => SnapshotIndexError::Corrupt,
            _ => SnapshotIndexError::Storage,
        },
    }
}

fn write_error(error: rusqlite::Error) -> SnapshotIndexError {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => SnapshotIndexError::Busy,
        Some(ErrorCode::DiskFull) => SnapshotIndexError::Storage,
        Some(ErrorCode::ConstraintViolation)
            if error.sqlite_error().map(|value| value.extended_code)
                == Some(rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER) =>
        {
            SnapshotIndexError::Storage
        }
        Some(ErrorCode::ConstraintViolation) => SnapshotIndexError::InvalidInput,
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => SnapshotIndexError::Corrupt,
        _ => SnapshotIndexError::Storage,
    }
}

#[cfg(test)]
mod tests;
