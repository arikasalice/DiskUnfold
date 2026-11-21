use super::*;
use crate::{HISTORY_DEFINITIONS, migration, snapshot_index::SNAPSHOT_INDEX_SCHEMA_VERSION};
use rusqlite::{Connection, OpenFlags, params};
use std::{
    sync::{Arc, Barrier},
    thread,
    time::Duration,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn id(value: u64) -> Id {
    let mut bytes = [0; 16];
    bytes[..6].copy_from_slice(&value.to_be_bytes()[2..]);
    bytes[15] = value as u8;
    Id::from_v7_bytes(bytes)
}

fn row(snapshot: u64, source: u64, observed_at_ms: Option<i64>) -> SnapshotIndexRow {
    SnapshotIndexRow {
        snapshot_id: id(snapshot),
        file_name: format!("snapshot-{snapshot}.dusnap"),
        source_version_id: id(source),
        lineage_id: id(900),
        source_kind: SnapshotIndexSourceKind::Local,
        observed_at_ms,
        created_at_ms: 10,
        scope_label: vec![0xAB; 32],
        condition_summary: r#"{"private":"condition"}"#.into(),
        completeness: Some(SnapshotIndexCompleteness::Complete),
        format_version: 1,
        body_size: 64,
        body_hash: [0x5A; 32],
        state: SnapshotIndexState::Active,
        verify_failed_at_ms: None,
        updated_at_ms: 10,
        row_version: 1,
    }
}

fn volume(index: u16) -> SnapshotIndexVolume {
    SnapshotIndexVolume {
        volume_index: index,
        observed_volume_id: None,
        identity_kind: SnapshotVolumeIdentityKind::None,
        identity_capability: SnapshotVolumeIdentityCapability::Unknown,
    }
}

fn installed(dir: &tempfile::TempDir) -> Result<AppDb, String> {
    AppDb::create_gui_with_history(&dir.path().join("app.db")).map_err(|error| format!("{error:?}"))
}

fn count(connection: &Connection, table: &str) -> rusqlite::Result<i64> {
    connection.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0)
    })
}

#[test]
fn register_is_idempotent_and_finds_the_active_source() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let first_row = row(1, 101, Some(100));
    let first = db.register(&first_row, &[volume(0)], None)?;
    let repeated = db.register(&row(2, 101, Some(200)), &[], Some(300))?;
    assert_eq!(first.snapshot_id, first_row.snapshot_id);
    assert_eq!(first.list_seq, 1);
    assert!(first.inserted);
    assert_eq!(repeated.snapshot_id, first_row.snapshot_id);
    assert_eq!(repeated.list_seq, 1);
    assert!(!repeated.inserted);
    assert_eq!(
        db.find_active_by_source(&id(101))?,
        Some(first_row.snapshot_id)
    );
    assert_eq!(count(&db.connection, "snapshot_store_snapshot")?, 1);
    assert_eq!(count(&db.connection, "snapshot_store_snapshot_volume")?, 1);
    assert_eq!(count(&db.connection, "snapshot_store_baseline")?, 0);
    Ok(())
}

#[test]
fn row_volumes_baseline_and_sequence_roll_back_on_a_constraint_failure() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let duplicate_volumes = [volume(4), volume(4)];
    assert_eq!(
        db.register(&row(1, 101, Some(100)), &duplicate_volumes, Some(200)),
        Err(SnapshotIndexError::InvalidInput)
    );
    assert_eq!(count(&db.connection, "snapshot_store_snapshot")?, 0);
    assert_eq!(count(&db.connection, "snapshot_store_snapshot_volume")?, 0);
    assert_eq!(count(&db.connection, "snapshot_store_baseline")?, 0);
    let seq: i64 = db.connection.query_row(
        "SELECT list_seq FROM snapshot_store_list_state WHERE name = 'snapshot_list'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(seq, 0);
    Ok(())
}

#[test]
fn baseline_is_inserted_with_its_snapshot_row() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let inserted = db.register(&row(1, 101, Some(100)), &[], Some(200))?;
    let pinned_at_ms: i64 = db.connection.query_row(
        "SELECT pinned_at_ms FROM snapshot_store_baseline WHERE snapshot_id = ?1",
        [inserted
            .snapshot_id
            .v7_bytes()
            .ok_or("invalid test id")?
            .as_slice()],
        |row| row.get(0),
    )?;
    assert_eq!(pinned_at_ms, 200);
    assert_eq!(inserted.list_seq, 1);
    Ok(())
}

#[test]
fn concurrent_connections_register_one_active_snapshot_for_the_source() -> TestResult {
    let dir = tempfile::tempdir()?;
    drop(installed(&dir)?);
    let path = dir.path().join("app.db");
    let barrier = Arc::new(Barrier::new(2));
    let workers = (1..=2)
        .map(|snapshot_id| {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || -> Result<SnapshotIndexRegisterResult, String> {
                let mut db =
                    AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
                barrier.wait();
                db.register(&row(snapshot_id, 101, Some(100)), &[], None)
                    .map_err(|error| format!("{error:?}"))
            })
        })
        .collect::<Vec<_>>();
    let outcomes = workers
        .into_iter()
        .map(|worker| {
            worker
                .join()
                .map_err(|_| std::io::Error::other("worker panicked"))?
                .map_err(std::io::Error::other)
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.inserted).count(),
        1
    );
    assert_eq!(outcomes[0].snapshot_id, outcomes[1].snapshot_id);
    let db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(count(&db.connection, "snapshot_store_snapshot")?, 1);
    let seq: i64 = db.connection.query_row(
        "SELECT list_seq FROM snapshot_store_list_state WHERE name = 'snapshot_list'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(seq, 1);
    Ok(())
}

#[test]
fn active_pages_sort_unknown_times_last_and_reject_stale_cursors() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    db.register(&row(10, 110, Some(100)), &[], None)?;
    db.register(&row(11, 111, None), &[], None)?;
    db.register(&row(12, 112, Some(300)), &[volume(0)], None)?;
    db.register(&row(13, 113, Some(300)), &[], Some(500))?;

    let first = db.list_active(2, None)?;
    assert_eq!(first.rows.len(), 2);
    assert_eq!(first.rows[0].row.snapshot_id, id(12));
    assert_eq!(first.rows[1].row.snapshot_id, id(13));
    assert!(first.rows[1].pinned);
    assert_eq!(first.rows[0].volumes, vec![volume(0)]);
    let cursor = first.next_cursor.ok_or("missing next page cursor")?;
    let second = db.list_active(2, Some(&cursor))?;
    assert_eq!(second.rows.len(), 2);
    assert_eq!(second.rows[0].row.snapshot_id, id(10));
    assert_eq!(second.rows[1].row.snapshot_id, id(11));
    assert_eq!(second.rows[1].row.observed_at_ms, None);
    assert_eq!(second.next_cursor, None);

    db.register(&row(14, 114, Some(400)), &[], None)?;
    assert_eq!(
        db.list_active(2, Some(&cursor)),
        Err(SnapshotIndexError::StaleCursor)
    );
    Ok(())
}

#[test]
fn volume_identity_writers_are_restricted_and_debug_redacts_sensitive_values() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let mut unsupported = volume(0);
    unsupported.identity_kind = SnapshotVolumeIdentityKind::Guid;
    unsupported.identity_capability = SnapshotVolumeIdentityCapability::Supported;
    assert_eq!(
        db.register(&row(1, 101, Some(100)), &[unsupported], None),
        Err(SnapshotIndexError::InvalidInput)
    );
    let mut sensitive_volume = volume(0);
    sensitive_volume.observed_volume_id = Some(b"raw-secret".to_vec());
    db.register(&row(1, 101, Some(100)), &[sensitive_volume.clone()], None)?;
    db.register(&row(2, 102, Some(90)), &[sensitive_volume.clone()], None)?;
    let page = db.list_active(10, None)?;
    assert_eq!(
        page.rows[0].volumes[0].observed_volume_id,
        Some(b"raw-secret".to_vec())
    );
    assert_eq!(
        page.rows[1].volumes[0].observed_volume_id,
        Some(b"raw-secret".to_vec())
    );
    let row_debug = format!("{:?}", row(1, 101, Some(100)));
    let volume_debug = format!("{sensitive_volume:?}");
    assert!(!row_debug.contains("private") && !row_debug.contains("171"));
    assert!(!volume_debug.contains("raw-secret"));
    Ok(())
}

#[test]
fn deletion_records_reject_update_and_delete() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let result = db.register(&row(1, 101, Some(100)), &[], None)?;
    let snapshot_id = result.snapshot_id.v7_bytes().ok_or("invalid test id")?;
    let deletion_id = id(700).v7_bytes().ok_or("invalid test id")?;
    db.connection.execute(
        "INSERT INTO snapshot_store_deletion
             (deletion_id, snapshot_id, deleted_at_ms, cause, file_removed)
         VALUES (?1, ?2, 300, 'manual', 1)",
        params![deletion_id.as_slice(), snapshot_id.as_slice()],
    )?;
    assert!(
        db.connection
            .execute(
                "UPDATE snapshot_store_deletion SET file_removed = 0 WHERE deletion_id = ?1",
                [deletion_id.as_slice()],
            )
            .is_err()
    );
    assert!(
        db.connection
            .execute(
                "DELETE FROM snapshot_store_deletion WHERE deletion_id = ?1",
                [deletion_id.as_slice()],
            )
            .is_err()
    );
    assert_eq!(count(&db.connection, "snapshot_store_deletion")?, 1);
    Ok(())
}

#[test]
fn list_sequence_overflow_rolls_back_the_snapshot_write() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    db.connection.execute(
        "UPDATE snapshot_store_list_state SET list_seq = ?1 WHERE name = 'snapshot_list'",
        [i64::MAX],
    )?;
    assert_eq!(
        db.register(&row(1, 101, Some(100)), &[], None),
        Err(SnapshotIndexError::Storage)
    );
    assert_eq!(count(&db.connection, "snapshot_store_snapshot")?, 0);
    let seq: i64 = db.connection.query_row(
        "SELECT list_seq FROM snapshot_store_list_state WHERE name = 'snapshot_list'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(seq, i64::MAX);
    Ok(())
}

#[test]
fn a_busy_immediate_write_returns_busy_without_inserting() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    db.connection.busy_timeout(Duration::ZERO)?;
    let blocker = Connection::open(dir.path().join("app.db"))?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    assert_eq!(
        db.register(&row(1, 101, Some(100)), &[], None),
        Err(SnapshotIndexError::Busy)
    );
    assert_eq!(count(&db.connection, "snapshot_store_snapshot")?, 0);
    blocker.execute_batch("ROLLBACK")?;
    Ok(())
}

#[test]
fn read_only_open_of_the_latest_old_schema_does_not_migrate() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("old.db");
    drop(AppDb::create_gui(&path).map_err(|error| format!("{error:?}"))?);
    for definition in HISTORY_DEFINITIONS
        .steps
        .iter()
        .filter(|definition| definition.number < SNAPSHOT_INDEX_SCHEMA_VERSION)
    {
        migration::apply_next(
            &path,
            &dir.path().join(format!("before-{}.db", definition.number)),
            &HISTORY_DEFINITIONS,
        )
        .map_err(|error| format!("{error:?}"))?;
    }
    let mut reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(reader.schema_version()?, SNAPSHOT_INDEX_SCHEMA_VERSION - 1);
    assert_eq!(
        reader.list_active(10, None),
        Err(SnapshotIndexError::UnsupportedSchema)
    );
    drop(reader);
    let check = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: u32 = check.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, SNAPSHOT_INDEX_SCHEMA_VERSION - 1);
    let table_exists: bool = check.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'snapshot_store_list_state')",
        [],
        |row| row.get(0),
    )?;
    assert!(!table_exists);
    Ok(())
}
