use super::*;
use du_core_contracts::{
    Id,
    job::{JobCounts, JobState, JobSummary},
};
use rusqlite::params;

fn id(n: u8) -> [u8; 16] {
    let mut value = [0; 16];
    value[15] = n;
    value
}

fn start(db: &AppDb, n: u8, predecessor: Option<u8>) -> rusqlite::Result<()> {
    let parent = predecessor.map(id);
    db.connection.execute(
        "INSERT INTO job_runtime_job_start
         (job_id, kind_id, kind_version, scope_summary, started_at_ms, plan_version_id,
          plan_lineage_id, plan_hash, predecessor_job_id, relation_kind, record_origin, created_at_ms)
         VALUES (?1, 'change.trash', 1, 'redacted', ?2, ?3, ?3, zeroblob(32), ?4, ?5, 'this_app', ?2)",
        params![id(n).as_slice(), i64::from(n), id(90).as_slice(), parent.as_ref().map(|v| v.as_slice()), predecessor.map(|_| "retry")],
    )?;
    Ok(())
}

fn terminal(db: &mut AppDb, n: u8) -> Result<(), String> {
    let job_id = Id::from_v7_bytes(id(n));
    let summary = JobSummary {
        job_id: job_id.clone(),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: JobCounts::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(1),
        finished_at_ms: 2,
        correlation_id: job_id,
    };
    db.append_scan_history(&summary, 3).map(|_| ())
}

#[test]
fn marks_only_unfinished_once_and_preserves_link_fields() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    start(&db, 1, Some(8))?;
    start(&db, 2, None)?;
    // Use the exact terminal ID bytes for the matching start row.
    let normal = *uuid::Uuid::parse_str(Id::from_v7_bytes(id(3)).as_str())?.as_bytes();
    db.connection.execute("INSERT INTO job_runtime_job_start (job_id, kind_id, kind_version, scope_summary, started_at_ms, record_origin, created_at_ms) VALUES (?1,'scan.metadata',1,'metadata: 1 root',1,'this_app',1)", [normal.as_slice()])?;
    terminal(&mut db, 3)?;
    let result = db.mark_interrupted_jobs(500)?.ok_or("missing receipt")?;
    assert_eq!(result.count, 2);
    assert_eq!(
        result.revision,
        HistoryRevision {
            delete_generation: 0,
            max_record_seq: 3
        }
    );
    assert_eq!(result.observed_at_ms, 500);
    let copied: (Vec<u8>, Vec<u8>, String, i64, i64, i64) = db.connection.query_row(
        "SELECT plan_version_id, predecessor_job_id, relation_kind, reckon_at_ms, record_seq, created_at_ms FROM job_runtime_job_link WHERE job_id=?1",
        [id(1).as_slice()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))?;
    assert_eq!(
        copied,
        (id(90).to_vec(), id(8).to_vec(), "retry".into(), 500, 2, 500)
    );
    drop(db);
    let mut db = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    assert!(db.mark_interrupted_jobs(700)?.is_none());
    let rows: (i64, i64, i64) = db.connection.query_row("SELECT (SELECT count(*) FROM job_runtime_job_interrupt), (SELECT count(*) FROM job_runtime_job_link), last_record_seq FROM job_runtime_history_state", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    assert_eq!(rows, (2, 3, 3));
    Ok(())
}

#[test]
fn link_failure_rolls_back_every_marker_and_revision() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("failure.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    start(&db, 1, None)?;
    start(&db, 2, None)?;
    db.connection.execute_batch("CREATE TRIGGER fail_second BEFORE INSERT ON job_runtime_job_link WHEN NEW.job_id = X'00000000000000000000000000000002' BEGIN SELECT RAISE(ABORT, 'test'); END;")?;
    assert!(db.mark_interrupted_jobs(500).is_err());
    let state: (i64, i64, i64, i64) = db.connection.query_row("SELECT (SELECT count(*) FROM job_runtime_job_interrupt), (SELECT count(*) FROM job_runtime_job_link), last_record_seq, row_version FROM job_runtime_history_state", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?;
    assert_eq!(state, (0, 0, 0, 1));
    db.connection.execute_batch("DROP TRIGGER fail_second")?;
    assert_eq!(
        db.mark_interrupted_jobs(600)?
            .ok_or("missing receipt")?
            .count,
        2
    );
    Ok(())
}
