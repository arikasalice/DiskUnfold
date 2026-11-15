//! Deleting history: a full purge and the deletion of one job take observations with
//! them; cutoff cleanup does not.

use super::*;
use du_core_contracts::job::{HistoryAppend, JobState, JobSummary};

const HISTORY_TABLES: [&str; 5] = [
    "job_runtime_item_result",
    "job_runtime_job_interrupt",
    "job_runtime_job_start",
    "job_runtime_job_record",
    "job_runtime_job_link",
];

fn finished_scan(n: u64, finished_at_ms: i64) -> JobSummary {
    JobSummary {
        job_id: id(n),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: Default::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(finished_at_ms - 1),
        finished_at_ms,
        correlation_id: id(n),
    }
}

fn history_rows(db: &AppDb) -> rusqlite::Result<i64> {
    db.connection.query_row(
        "SELECT (SELECT count(*) FROM job_runtime_job_record) + (SELECT count(*) FROM job_runtime_job_link)",
        [],
        |row| row.get(0),
    )
}

fn seeded(dir: &tempfile::TempDir) -> Result<AppDb, Box<dyn std::error::Error>> {
    let mut db = installed(dir)?;
    for (job, at) in [(1u64, 1_000i64), (2, 5_000)] {
        assert_eq!(
            db.append_scan_history(&finished_scan(job, at), at),
            Ok(HistoryAppend::Added)
        );
        db.append_scope_observation(&observation(job, 1, at))
            .map_err(|e| format!("{e:?}"))?;
    }
    assert_eq!((history_rows(&db)?, count(&db)?), (4, 2));
    Ok(db)
}

#[test]
fn cleanup_before_a_cutoff_removes_history_but_not_observations() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = seeded(&dir)?;
    db.connection.execute_batch(
        "BEGIN IMMEDIATE;
         UPDATE app_db_purge_gate SET open = 1;
         DELETE FROM job_runtime_job_link WHERE reckon_at_ms < 3000;
         DELETE FROM job_runtime_job_record WHERE finished_at_ms < 3000;
         UPDATE app_db_purge_gate SET open = 0;
         COMMIT;",
    )?;
    assert_eq!(history_rows(&db)?, 2);
    assert_eq!(count(&db)?, 2);
    Ok(())
}

#[test]
fn a_full_history_purge_removes_observations_in_the_same_transaction() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = seeded(&dir)?;
    let purge = |db: &mut AppDb, commit: bool| -> rusqlite::Result<()> {
        let tx = db.connection.transaction()?;
        tx.execute("UPDATE app_db_purge_gate SET open = 1", [])?;
        for table in HISTORY_TABLES {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
        assert_eq!(delete_all_scope_observations(&tx)?, 2);
        tx.execute("UPDATE app_db_purge_gate SET open = 0", [])?;
        if commit {
            tx.commit()?;
        }
        Ok(())
    };
    // A purge that does not commit changes nothing.
    purge(&mut db, false)?;
    assert_eq!((history_rows(&db)?, count(&db)?), (4, 2));
    purge(&mut db, true)?;
    assert_eq!((history_rows(&db)?, count(&db)?), (0, 0));
    Ok(())
}

#[test]
fn the_helper_cannot_delete_while_the_gate_is_closed() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = seeded(&dir)?;
    let tx = db.connection.transaction()?;
    assert!(delete_all_scope_observations(&tx).is_err());
    drop(tx);
    assert_eq!(count(&db)?, 2);
    Ok(())
}

fn observed_jobs(db: &AppDb) -> rusqlite::Result<Vec<Vec<u8>>> {
    let mut statement = db
        .connection
        .prepare("SELECT job_id FROM analysis_index_scope_observation ORDER BY job_id")?;
    let rows = statement.query_map([], |row| row.get(0))?;
    rows.collect()
}

#[test]
fn deleting_one_jobs_history_removes_only_that_jobs_observation() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = seeded(&dir)?;
    let job = id(1);
    let key = job.v7_bytes().ok_or("job id")?;
    let delete = |db: &mut AppDb, commit: bool| -> rusqlite::Result<usize> {
        let tx = db.connection.transaction()?;
        tx.execute("UPDATE app_db_purge_gate SET open = 1", [])?;
        for table in ["job_runtime_job_record", "job_runtime_job_link"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE job_id = ?1"),
                [key.as_slice()],
            )?;
        }
        let removed = delete_scope_observation_of_job(&tx, &job)?;
        tx.execute("UPDATE app_db_purge_gate SET open = 0", [])?;
        if commit {
            tx.commit()?;
        }
        Ok(removed)
    };
    // A deletion that does not commit changes nothing.
    assert_eq!(delete(&mut db, false)?, 1);
    assert_eq!((history_rows(&db)?, count(&db)?), (4, 2));
    assert_eq!(delete(&mut db, true)?, 1);
    assert_eq!((history_rows(&db)?, count(&db)?), (2, 1));
    let other = id(2).v7_bytes().ok_or("job id")?;
    assert_eq!(observed_jobs(&db)?, vec![other.to_vec()]);
    // The job has no observation any more: deleting again removes nothing.
    assert_eq!(delete(&mut db, true)?, 0);
    assert_eq!(count(&db)?, 1);
    Ok(())
}

#[test]
fn a_job_without_an_observation_deletes_nothing() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = seeded(&dir)?;
    let tx = db.connection.transaction()?;
    tx.execute("UPDATE app_db_purge_gate SET open = 1", [])?;
    assert_eq!(delete_scope_observation_of_job(&tx, &id(9))?, 0);
    tx.execute("UPDATE app_db_purge_gate SET open = 0", [])?;
    tx.commit()?;
    assert_eq!(count(&db)?, 2);
    Ok(())
}

#[test]
fn the_job_helper_cannot_delete_while_the_gate_is_closed() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = seeded(&dir)?;
    let tx = db.connection.transaction()?;
    assert!(delete_scope_observation_of_job(&tx, &id(1)).is_err());
    drop(tx);
    assert_eq!(count(&db)?, 2);
    Ok(())
}

#[test]
fn the_job_deletion_reads_the_unique_job_index_and_not_the_table() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = seeded(&dir)?;
    let key = id(1).v7_bytes().ok_or("job id")?;
    let mut statement = db.connection.prepare(
        "EXPLAIN QUERY PLAN DELETE FROM analysis_index_scope_observation WHERE job_id = ?1",
    )?;
    let plan: Vec<String> = statement
        .query_map([key.as_slice()], |row| row.get(3))?
        .collect::<rusqlite::Result<_>>()?;
    assert!(
        plan.iter()
            .any(|step| step.contains("SEARCH") && step.contains("job_id")),
        "{plan:?}"
    );
    assert!(plan.iter().all(|step| !step.contains("SCAN")), "{plan:?}");
    Ok(())
}
