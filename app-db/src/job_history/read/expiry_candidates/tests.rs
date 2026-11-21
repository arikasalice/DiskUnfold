use super::*;
use rusqlite::{Connection, params};
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn writer(path: &Path) -> Result<AppDb, Box<dyn std::error::Error>> {
    AppDb::create_gui_with_history(path).map_err(|error| format!("{error:?}").into())
}

fn reader(path: &Path) -> Result<AppDb, Box<dyn std::error::Error>> {
    AppDb::open_read_only_live(path).map_err(|error| format!("{error:?}").into())
}

fn insert_link(
    db: &AppDb,
    job_id: [u8; 16],
    reckon_at_ms: i64,
    terminal: bool,
    interrupted: bool,
) -> Result<(), rusqlite::Error> {
    db.connection.execute(
        "INSERT INTO job_runtime_job_link
             (job_id, record_origin, reckon_at_ms, record_seq, created_at_ms)
         VALUES (?1, 'this_app', ?2, ?3, ?2)",
        params![job_id.as_slice(), reckon_at_ms, i64::from(job_id[0]) + 1],
    )?;
    if terminal {
        db.connection.execute(
            "INSERT INTO job_runtime_job_record
                 (job_id, kind_id, kind_version, scope_summary, final_state,
                  completed_count, overwritten_count, failed_count, skipped_count,
                  unprocessed_count, started_at_ms, finished_at_ms, correlation_id, created_at_ms)
             VALUES (?1, 'scan.metadata', 1, 'metadata: 1 root', 'done', 0, 0, 0, 0, 0, ?2, ?2, ?1, ?2)",
            params![job_id.as_slice(), reckon_at_ms],
        )?;
    } else if interrupted {
        db.connection.execute(
            "INSERT INTO job_runtime_job_start
                 (job_id, kind_id, kind_version, scope_summary, started_at_ms, record_origin, created_at_ms)
             VALUES (?1, 'change.trash', 1, 'change.trash', ?2, 'this_app', ?2)",
            params![job_id.as_slice(), reckon_at_ms],
        )?;
        db.connection.execute(
            "INSERT INTO job_runtime_job_interrupt VALUES (?1, ?2)",
            params![job_id.as_slice(), reckon_at_ms],
        )?;
    }
    Ok(())
}

fn expected_digest(ids: &[[u8; 16]]) -> [u8; 32] {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    let mut hasher = blake3::Hasher::new();
    for id in sorted {
        hasher.update(&id);
    }
    *hasher.finalize().as_bytes()
}

fn plan_details(connection: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut statement = connection.prepare(&format!(
        "EXPLAIN QUERY PLAN {}",
        include_str!("../expiry_candidates.sql")
    ))?;
    let rows = statement.query_map(params![100_i64, 200_i64], |row| row.get(3))?;
    rows.collect()
}

#[test]
fn expiry_candidates_apply_boundaries_terminal_rules_and_digest() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let writer_db = writer(&path)?;
    let reference_ms = 86_400_100;
    let candidates = [[3; 16], [1; 16], [2; 16]];
    insert_link(&writer_db, candidates[0], 100, true, false)?;
    insert_link(&writer_db, candidates[1], 90, true, false)?;
    insert_link(&writer_db, candidates[2], 100, false, true)?;
    insert_link(&writer_db, [4; 16], 101, true, false)?;
    insert_link(&writer_db, [5; 16], reference_ms + 1, true, false)?;
    insert_link(&writer_db, [6; 16], 80, false, false)?;

    let mut reader = reader(&path)?;
    let result = reader.read_history_expiry_candidates(1, reference_ms)?;
    assert_eq!(result.count, 3);
    assert_eq!(result.earliest_reckon_at_ms, Some(90));
    assert_eq!(result.latest_reckon_at_ms, Some(100));
    assert_eq!(result.digest, expected_digest(&candidates));
    Ok(())
}

#[test]
fn expiry_digest_is_insert_order_independent_and_changes_with_the_set() -> TestResult {
    let dir = tempfile::tempdir()?;
    let first_path = dir.path().join("first.db");
    let second_path = dir.path().join("second.db");
    let ids = [[7; 16], [2; 16], [9; 16]];
    for (path, order) in [(&first_path, [0, 1, 2]), (&second_path, [2, 0, 1])] {
        let writer = writer(path)?;
        for index in order {
            insert_link(&writer, ids[index], 10 + i64::try_from(index)?, true, false)?;
        }
    }
    let mut first_reader = reader(&first_path)?;
    let first = first_reader.read_history_expiry_candidates(1, 100)?;
    let mut second_reader = reader(&second_path)?;
    let second = second_reader.read_history_expiry_candidates(1, 100)?;
    assert_eq!(first.digest, second.digest);
    let fewer = first_reader.read_history_expiry_candidates(0, 11)?;
    assert_eq!(fewer.count, 2);
    assert_ne!(first.digest, fewer.digest);
    Ok(())
}

#[test]
fn huge_retention_period_and_empty_history_return_empty_range() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let writer_db = writer(&path)?;
    insert_link(&writer_db, [1; 16], 0, true, false)?;
    let mut reader_db = reader(&path)?;
    let huge = reader_db.read_history_expiry_candidates(u32::MAX, 0)?;
    assert_eq!(huge.count, 0);
    assert_eq!(huge.earliest_reckon_at_ms, None);
    assert_eq!(huge.latest_reckon_at_ms, None);

    let empty_path = dir.path().join("empty.db");
    let empty_writer = writer(&empty_path)?;
    drop(empty_writer);
    let empty = reader(&empty_path)?.read_history_expiry_candidates(30, 100)?;
    assert_eq!(empty.count, 0);
    assert_eq!(empty.earliest_reckon_at_ms, None);
    assert_eq!(empty.latest_reckon_at_ms, None);
    assert_eq!(empty.digest, *blake3::hash(&[]).as_bytes());
    Ok(())
}

#[test]
fn expiry_query_uses_reckon_index_and_requires_a_read_connection() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let mut writer = writer(&path)?;
    assert!(matches!(
        writer.read_history_expiry_candidates(1, 100),
        Err(Error::RequiresReadConnection)
    ));
    assert_eq!(
        writer.expiry_candidates(1, 100),
        Err(HistoryExpiryReadError::Unavailable)
    );
    let plan = plan_details(&writer.connection)?;
    println!("expiry candidates EXPLAIN QUERY PLAN: {plan:?}");
    assert!(
        plan.iter().any(|line| {
            line.contains("SEARCH link USING INDEX ix_job_runtime_job_link_reckon")
        })
    );
    assert!(!plan.iter().any(|line| line.contains("SCAN link")));

    let mut reader = reader(&path)?;
    assert!(matches!(
        reader.expiry_candidates(1, 100),
        Ok(HistoryExpiryCandidates { count: 0, .. })
    ));
    Ok(())
}
