use super::*;
use du_app_db::AppDb;
use du_core_contracts::{
    Id, SizeEvidence,
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
    result::ResultCompleteness,
    scope_observation_log::{
        ObservedAmount, ObservedSize, SCOPE_FINGERPRINT_FORMAT_V1, ScopeObservationOrigin,
        ScopeObservationRead,
    },
};
use rusqlite::Connection;
use std::sync::Arc;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn shared(path: &std::path::Path) -> Result<SharedDatabase, Box<dyn std::error::Error>> {
    let db = AppDb::create_gui_with_history(path).map_err(|e| format!("{e:?}"))?;
    Ok(SharedDatabase::new(db, None))
}

fn observation(job: u8) -> ScopeObservation {
    let size = |bytes| ObservedSize {
        amount: ObservedAmount::Exact(bytes),
        evidence: SizeEvidence::Measured,
    };
    ScopeObservation {
        observation_id: Id::from_v7_bytes([job.wrapping_add(100); 16]),
        scope_fingerprint: [7; 32],
        fingerprint_format: SCOPE_FINGERPRINT_FORMAT_V1,
        observed_at_ms: 1_000 + i64::from(job),
        job_id: Id::from_v7_bytes([job; 16]),
        origin: ScopeObservationOrigin::Scan,
        completeness: ResultCompleteness::Complete,
        size_rule_id: "size.agg.subtree-unique".into(),
        size_rule_revision: 1,
        logical: size(10),
        allocated: size(20),
        unique_physical: ObservedSize {
            amount: ObservedAmount::Unknown,
            evidence: SizeEvidence::Unknown,
        },
        created_at_ms: 2_000,
    }
}

fn scan_summary(job: u8) -> JobSummary {
    JobSummary {
        job_id: Id::from_v7_bytes([job; 16]),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: Default::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(100),
        finished_at_ms: 200,
        correlation_id: Id::from_v7_bytes([job; 16]),
    }
}

#[test]
fn the_log_writes_on_the_shared_connection_and_is_idempotent_per_job() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("shared.db");
    let database = shared(&path)?;
    let log: Arc<dyn ScopeObservationLog> = Arc::new(database.scope_observations());
    assert_eq!(
        log.append(&observation(1)),
        Ok(ScopeObservationAppend::Added)
    );
    assert_eq!(
        log.append(&observation(1)),
        Ok(ScopeObservationAppend::AlreadyRecorded)
    );
    let reader = AppDb::open_read_only_live(&path).map_err(|e| format!("{e:?}"))?;
    let listed = reader
        .list(&[7; 32], SCOPE_FINGERPRINT_FORMAT_V1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(listed.rows, [observation(1)]);
    Ok(())
}

#[test]
fn a_refused_observation_leaves_the_scan_history_record_intact() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("shared.db");
    let database = shared(&path)?;
    let history = database.history(|| 250);
    let log = database.scope_observations();
    Connection::open(&path)?.execute_batch(
        "CREATE TRIGGER reject_observation BEFORE INSERT ON analysis_index_scope_observation
         BEGIN SELECT RAISE(ABORT, 'test'); END;",
    )?;
    assert_eq!(history.append(&scan_summary(1)), Ok(HistoryAppend::Added));
    assert_eq!(
        log.append(&observation(1)),
        Err(ScopeObservationWriteError::Storage)
    );
    let sql = Connection::open(&path)?;
    let (records, links, observations): (i64, i64, i64) = sql.query_row(
        "SELECT (SELECT count(*) FROM job_runtime_job_record),
                (SELECT count(*) FROM job_runtime_job_link),
                (SELECT count(*) FROM analysis_index_scope_observation)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!((records, links, observations), (1, 1, 0));
    // The shared connection stays usable for the next scan.
    sql.execute_batch("DROP TRIGGER reject_observation")?;
    assert_eq!(
        log.append(&observation(2)),
        Ok(ScopeObservationAppend::Added)
    );
    assert_eq!(history.append(&scan_summary(2)), Ok(HistoryAppend::Added));
    Ok(())
}
