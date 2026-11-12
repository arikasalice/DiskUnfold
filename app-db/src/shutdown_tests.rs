use super::{AppDb, WalCheckpointError};

#[test]
fn truncate_checkpoint_reports_frames_and_database_reopens()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("checkpoint.db");
    let mut writer = AppDb::create_gui(&path).map_err(|error| format!("create: {error:?}"))?;
    writer.connection.execute_batch(
        "CREATE TABLE shutdown_checkpoint_probe(value INTEGER NOT NULL);
         INSERT INTO shutdown_checkpoint_probe(value) VALUES (1), (2), (3);",
    )?;

    let result = writer.checkpoint_wal()?;

    assert!(result.log_frames.is_none_or(|frames| frames >= 0));
    assert!(result.checkpointed_frames.is_none_or(|frames| frames >= 0));
    drop(writer);
    let reopened = AppDb::open_gui(&path).map_err(|error| format!("reopen: {error:?}"))?;
    let rows: i64 = reopened.connection.query_row(
        "SELECT count(*) FROM shutdown_checkpoint_probe",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(rows, 3);
    Ok(())
}

#[test]
fn truncate_checkpoint_reports_a_live_reader_instead_of_claiming_success()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("busy-checkpoint.db");
    let mut writer = AppDb::create_gui(&path).map_err(|error| format!("create: {error:?}"))?;
    writer.connection.execute_batch(
        "CREATE TABLE shutdown_checkpoint_probe(value INTEGER NOT NULL);
         INSERT INTO shutdown_checkpoint_probe(value) VALUES (1);",
    )?;
    let mut reader = AppDb::open_read_only(&path).map_err(|error| format!("reader: {error:?}"))?;
    reader.connection.busy_timeout(std::time::Duration::ZERO)?;
    let snapshot = reader.connection.transaction()?;
    let _: i64 = snapshot.query_row(
        "SELECT count(*) FROM shutdown_checkpoint_probe",
        [],
        |row| row.get(0),
    )?;
    writer.connection.execute(
        "INSERT INTO shutdown_checkpoint_probe(value) VALUES (?1)",
        [2],
    )?;

    let error = writer
        .checkpoint_wal()
        .err()
        .ok_or("checkpoint unexpectedly succeeded while a reader pinned the WAL")?;
    assert!(matches!(error, WalCheckpointError::Busy { .. }));

    drop(snapshot);
    writer.checkpoint_wal()?;
    Ok(())
}

#[test]
fn truncate_checkpoint_reports_a_concurrent_writer_instead_of_claiming_success()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("busy-writer-checkpoint.db");
    let mut checkpoint = AppDb::create_gui(&path).map_err(|error| format!("create: {error:?}"))?;
    checkpoint
        .connection
        .busy_timeout(std::time::Duration::ZERO)?;
    checkpoint.connection.execute_batch(
        "CREATE TABLE shutdown_checkpoint_probe(value INTEGER NOT NULL);
         INSERT INTO shutdown_checkpoint_probe(value) VALUES (1);",
    )?;
    let mut external_writer = rusqlite::Connection::open(&path)?;
    external_writer.busy_timeout(std::time::Duration::ZERO)?;
    let write =
        external_writer.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    write.execute(
        "INSERT INTO shutdown_checkpoint_probe(value) VALUES (2)",
        [],
    )?;

    let error = checkpoint
        .checkpoint_wal()
        .err()
        .ok_or("checkpoint unexpectedly succeeded while a writer held the WAL")?;
    assert!(matches!(error, WalCheckpointError::Busy { .. }));

    drop(write);
    checkpoint.checkpoint_wal()?;
    Ok(())
}
