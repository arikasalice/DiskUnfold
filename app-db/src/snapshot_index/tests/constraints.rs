use super::*;
use rusqlite::params;

#[test]
fn numeric_checks_reject_negative_times_sizes_and_invalid_versions() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;

    let mut invalid = row(100, 200, Some(1));
    invalid.created_at_ms = -1;
    assert_eq!(
        db.register(&invalid, &[], None),
        Err(SnapshotIndexError::InvalidInput)
    );

    let invalid = row(101, 201, Some(-1));
    assert!(insert_snapshot(&db, &invalid).is_err());
    let mut invalid = row(102, 202, Some(1));
    invalid.created_at_ms = -1;
    assert!(insert_snapshot(&db, &invalid).is_err());
    let mut invalid = row(103, 203, Some(1));
    invalid.body_size = -1;
    assert!(insert_snapshot(&db, &invalid).is_err());
    let mut invalid = row(104, 204, Some(1));
    invalid.verify_failed_at_ms = Some(-1);
    assert!(insert_snapshot(&db, &invalid).is_err());
    let mut invalid = row(105, 205, Some(1));
    invalid.updated_at_ms = -1;
    assert!(insert_snapshot(&db, &invalid).is_err());
    let mut invalid = row(106, 206, Some(1));
    invalid.row_version = 0;
    assert!(insert_snapshot(&db, &invalid).is_err());
    let mut invalid = row(108, 208, Some(1));
    invalid.condition_summary = "not-json".into();
    assert!(insert_snapshot(&db, &invalid).is_err());
    let mut invalid = row(109, 209, Some(1));
    invalid.condition_summary = format!("\"{}\"", "x".repeat(65_536));
    assert!(insert_snapshot(&db, &invalid).is_err());

    let parent = row(107, 207, Some(1));
    insert_snapshot(&db, &parent)?;
    let snapshot_id = parent.snapshot_id.v7_bytes().ok_or("invalid snapshot id")?;
    assert!(
        db.connection
            .execute(
                "INSERT INTO snapshot_store_baseline(snapshot_id, pinned_at_ms)
             VALUES (?1, -1)",
                [snapshot_id.as_slice()],
            )
            .is_err()
    );
    assert!(
        db.connection
            .execute(
                "INSERT INTO snapshot_store_deletion
                 (deletion_id, snapshot_id, deleted_at_ms, cause, file_removed)
             VALUES (?1, ?2, -1, 'manual', 1)",
                params![
                    id(900).v7_bytes().ok_or("invalid deletion id")?.as_slice(),
                    snapshot_id.as_slice()
                ],
            )
            .is_err()
    );
    Ok(())
}

fn insert_snapshot(db: &AppDb, row: &SnapshotIndexRow) -> rusqlite::Result<usize> {
    let snapshot_id = row
        .snapshot_id
        .v7_bytes()
        .ok_or(rusqlite::Error::InvalidQuery)?;
    let source_version_id = row
        .source_version_id
        .v7_bytes()
        .ok_or(rusqlite::Error::InvalidQuery)?;
    let lineage_id = row
        .lineage_id
        .v7_bytes()
        .ok_or(rusqlite::Error::InvalidQuery)?;
    db.connection.execute(
        "INSERT INTO snapshot_store_snapshot (
             snapshot_id, file_name, source_version_id, lineage_id, source_kind,
             observed_at_ms, created_at_ms, scope_label, condition_summary,
             completeness, format_version, body_size, body_hash, state,
             verify_failed_at_ms, updated_at_ms, row_version)
         VALUES (?1, ?2, ?3, ?4, 'local', ?5, ?6, ?7, ?8, 'complete', ?9,
                 ?10, ?11, 'active', ?12, ?13, ?14)",
        params![
            snapshot_id.as_slice(),
            row.file_name,
            source_version_id.as_slice(),
            lineage_id.as_slice(),
            row.observed_at_ms,
            row.created_at_ms,
            row.scope_label,
            row.condition_summary,
            i64::from(row.format_version),
            row.body_size,
            row.body_hash.as_slice(),
            row.verify_failed_at_ms,
            row.updated_at_ms,
            row.row_version,
        ],
    )
}
