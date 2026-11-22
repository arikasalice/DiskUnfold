use du_core_contracts::snapshot_index::{
    SnapshotIndexCursor, SnapshotIndexEntry, SnapshotIndexError,
};
use rusqlite::{ToSql, Transaction};

use super::read_error;

const LIST_FIELDS: &str = "snapshot_id, file_name, source_version_id, lineage_id, source_kind,
    observed_at_ms, created_at_ms, scope_label, condition_summary, completeness,
    format_version, body_size, body_hash, state, verify_failed_at_ms, updated_at_ms, row_version";

pub(super) fn list_sql(after: Option<&SnapshotIndexCursor>) -> String {
    let (predicate, limit_parameter) = match after {
        None => ("", "?1"),
        Some(cursor) if cursor.observed_at_ms.is_some() => {
            (" AND s.observed_at_ms = ?1 AND s.snapshot_id > ?2", "?3")
        }
        Some(_) => (" AND s.observed_at_ms IS NULL AND s.snapshot_id > ?1", "?2"),
    };
    list_query(predicate, limit_parameter)
}

pub(super) fn list_known_older_sql() -> String {
    list_query(" AND s.observed_at_ms < ?1", "?2")
}

pub(super) fn list_null_tail_sql() -> String {
    list_query(" AND s.observed_at_ms IS NULL", "?1")
}

fn list_query(predicate: &str, limit_parameter: &str) -> String {
    // SQLite orders NULL after numeric values for DESC, matching the cursor contract.
    format!(
        "SELECT {LIST_FIELDS}, EXISTS(
             SELECT 1 FROM snapshot_store_baseline b
             WHERE b.snapshot_id = s.snapshot_id)
         FROM snapshot_store_snapshot s
         WHERE s.state = 'active'{predicate}
         ORDER BY s.observed_at_ms DESC, s.snapshot_id ASC
         LIMIT {limit_parameter}"
    )
}

pub(super) fn append_list_rows(
    tx: &Transaction<'_>,
    sql: &str,
    parameters: &[&dyn ToSql],
    entries: &mut Vec<SnapshotIndexEntry>,
) -> Result<(), SnapshotIndexError> {
    let mut statement = tx.prepare_cached(sql).map_err(read_error)?;
    let mut rows = statement.query(parameters).map_err(read_error)?;
    while let Some(row) = rows.next().map_err(read_error)? {
        let snapshot = super::decode_snapshot(row).map_err(read_error)?;
        let pinned: bool = row.get(17).map_err(read_error)?;
        entries.push(SnapshotIndexEntry {
            row: snapshot,
            volumes: Vec::new(),
            pinned,
        });
    }
    Ok(())
}
