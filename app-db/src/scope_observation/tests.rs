use super::*;
use crate::{HISTORY_DEFINITIONS, migration};
use du_core_contracts::{
    Id, SizeEvidence,
    result::ResultCompleteness,
    scope_observation_log::{
        ObservedAmount, ObservedSize, SCOPE_FINGERPRINT_FORMAT_V1, ScopeObservation,
        ScopeObservationAppend, ScopeObservationOrigin, ScopeObservationRead,
        ScopeObservationWriteError,
    },
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

mod plan;
mod purge;
mod read;
mod retention;
mod upgrade;

pub(super) type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A UUID v7 whose bytes sort by `n`.
pub(super) fn id(n: u64) -> Id {
    let mut bytes = [0u8; 16];
    bytes[..6].copy_from_slice(&n.to_be_bytes()[2..]);
    bytes[10..14].copy_from_slice(&0x5a5a_5a5au32.to_be_bytes());
    Id::from_v7_bytes(bytes)
}

pub(super) fn fingerprint(scope: u16) -> [u8; 32] {
    let mut value = [0xA5; 32];
    value[..2].copy_from_slice(&scope.to_be_bytes());
    value
}

pub(super) fn size(bytes: u64) -> ObservedSize {
    ObservedSize {
        amount: ObservedAmount::Exact(bytes),
        evidence: SizeEvidence::Measured,
    }
}

/// A complete scan observation of scope `scope` for job `job` taken at `at`.
pub(super) fn observation(job: u64, scope: u16, at: i64) -> ScopeObservation {
    ScopeObservation {
        observation_id: id(job + 1_000_000),
        scope_fingerprint: fingerprint(scope),
        fingerprint_format: SCOPE_FINGERPRINT_FORMAT_V1,
        observed_at_ms: at,
        job_id: id(job),
        origin: ScopeObservationOrigin::Scan,
        completeness: ResultCompleteness::Complete,
        size_rule_id: "size.agg.subtree-unique".into(),
        size_rule_revision: 1,
        logical: size(4_000),
        allocated: size(8_192),
        unique_physical: ObservedSize {
            amount: ObservedAmount::Unknown,
            evidence: SizeEvidence::Unknown,
        },
        created_at_ms: at + 5,
    }
}

pub(super) fn installed(dir: &tempfile::TempDir) -> Result<AppDb, String> {
    AppDb::create_gui_with_history(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))
}

pub(super) fn count(db: &AppDb) -> rusqlite::Result<i64> {
    db.connection.query_row(
        "SELECT count(*) FROM analysis_index_scope_observation",
        [],
        |row| row.get(0),
    )
}

/// Inserts `rows` valid rows for one scope with one SQL statement, bypassing retention.
pub(super) fn bulk_insert(
    db: &AppDb,
    scope: u16,
    rows: i64,
    first_at_ms: i64,
    step_ms: i64,
) -> rusqlite::Result<()> {
    db.connection
        .execute(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < ?3 - 1)
             INSERT INTO analysis_index_scope_observation
                (observation_id, scope_fingerprint, fingerprint_format, observed_at_ms, job_id,
                 origin, completeness, size_rule_id, size_rule_revision,
                 logical_state, logical_bytes, logical_evidence,
                 allocated_state, allocated_bytes, allocated_evidence,
                 unique_physical_state, unique_physical_bytes, unique_physical_evidence,
                 created_at_ms)
             SELECT CAST(randomblob(6) || x'7000' || x'80' || randomblob(7) AS BLOB), ?1, 1,
                    ?2 + i * ?4,
                    CAST(randomblob(6) || x'7000' || x'80' || randomblob(7) AS BLOB),
                    'scan', 'complete', 'size.agg.subtree-unique', 1,
                    'exact', i, 'measured', 'exact', i, 'measured', 'unknown', NULL, 'unknown', 0
             FROM n",
            rusqlite::params![fingerprint(scope).as_slice(), first_at_ms, rows, step_ms],
        )
        .map(|_| ())
}

#[test]
fn stores_and_lists_one_observation() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let stored = observation(1, 1, 10_000);
    assert_eq!(
        db.append_scope_observation(&stored),
        Ok(ScopeObservationAppend::Added)
    );
    let listed = db
        .list(&fingerprint(1), SCOPE_FINGERPRINT_FORMAT_V1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(listed.rows, [stored]);
    assert!(!listed.truncated);
    Ok(())
}

#[test]
fn a_second_append_for_the_same_job_is_already_recorded() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let first = observation(1, 1, 10_000);
    assert_eq!(
        db.append_scope_observation(&first),
        Ok(ScopeObservationAppend::Added)
    );
    // The identical row, and a different row for the same job, both leave the first one.
    let mut other = observation(1, 2, 99_000);
    other.observation_id = id(77);
    for repeated in [&first, &other] {
        assert_eq!(
            db.append_scope_observation(repeated),
            Ok(ScopeObservationAppend::AlreadyRecorded)
        );
    }
    assert_eq!(count(&db)?, 1);
    let listed = db
        .list(&fingerprint(1), 1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(listed.rows, [first]);
    Ok(())
}

#[test]
fn byte_boundaries_and_unknown_amounts_round_trip() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let mut zero = observation(1, 1, 1_000);
    zero.logical = size(0);
    zero.allocated = ObservedSize {
        amount: ObservedAmount::LowerBound(0),
        evidence: SizeEvidence::OsReported,
    };
    let mut largest = observation(2, 1, 2_000);
    largest.logical = size(i64::MAX as u64);
    largest.allocated = ObservedSize {
        amount: ObservedAmount::LowerBound(i64::MAX as u64),
        evidence: SizeEvidence::Estimated,
    };
    let mut partial = observation(3, 1, 3_000);
    partial.completeness = ResultCompleteness::Partial;
    partial.logical = ObservedSize {
        amount: ObservedAmount::LowerBound(123),
        evidence: SizeEvidence::Measured,
    };
    let mut unknown = observation(4, 1, 4_000);
    unknown.completeness = ResultCompleteness::Missing;
    unknown.logical = ObservedSize {
        amount: ObservedAmount::Unknown,
        evidence: SizeEvidence::Unknown,
    };
    let rows = [zero, largest, partial, unknown];
    for row in &rows {
        assert_eq!(
            db.append_scope_observation(row),
            Ok(ScopeObservationAppend::Added)
        );
    }
    let listed = db
        .list(&fingerprint(1), 1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(listed.rows, rows);
    // Unknown is stored as an absent count, never as zero.
    let (state, bytes): (String, Option<i64>) = db.connection.query_row(
        "SELECT logical_state, logical_bytes FROM analysis_index_scope_observation WHERE observed_at_ms = 4000",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!((state.as_str(), bytes), ("unknown", None));
    Ok(())
}

#[test]
fn counts_a_sqlite_integer_cannot_hold_fail_instead_of_becoming_unknown() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let too_large = [1u64 << 63, u64::MAX];
    for bytes in too_large {
        for field in 0..3 {
            for amount in [
                ObservedAmount::Exact(bytes),
                ObservedAmount::LowerBound(bytes),
            ] {
                let mut row = observation(1, 1, 1_000);
                let target = ObservedSize {
                    amount,
                    evidence: SizeEvidence::Measured,
                };
                match field {
                    0 => row.logical = target,
                    1 => row.allocated = target,
                    _ => row.unique_physical = target,
                }
                assert_eq!(
                    db.append_scope_observation(&row),
                    Err(ScopeObservationWriteError::Invalid)
                );
            }
        }
    }
    assert_eq!(count(&db)?, 0);
    Ok(())
}

#[test]
fn values_the_table_cannot_represent_are_refused_before_sql() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let mut format = observation(1, 1, 1_000);
    format.fingerprint_format = 0;
    let mut revision = observation(2, 1, 1_000);
    revision.size_rule_revision = 0;
    let mut rule = observation(3, 1, 1_000);
    rule.size_rule_id.clear();
    for row in [format, revision, rule] {
        assert_eq!(
            db.append_scope_observation(&row),
            Err(ScopeObservationWriteError::Invalid)
        );
    }
    assert_eq!(count(&db)?, 0);
    Ok(())
}

#[test]
fn table_constraints_reject_inconsistent_amounts_and_unknown_names() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = installed(&dir)?;
    let insert = |tail: &str| {
        db.connection.execute(
            &format!(
                "INSERT INTO analysis_index_scope_observation
                 (observation_id, scope_fingerprint, fingerprint_format, observed_at_ms, job_id,
                  origin, completeness, size_rule_id, size_rule_revision,
                  logical_state, logical_bytes, logical_evidence,
                  allocated_state, allocated_bytes, allocated_evidence,
                  unique_physical_state, unique_physical_bytes, unique_physical_evidence,
                  created_at_ms)
                 VALUES (randomblob(16), randomblob(32), 1, 1, randomblob(16),
                  {tail}, 0)"
            ),
            [],
        )
    };
    let ok = "'scan','complete','r',1,'exact',1,'measured','exact',1,'measured','unknown',NULL,'unknown'";
    assert!(insert(ok).is_ok());
    for bad in [
        "'scan','complete','r',1,'unknown',1,'measured','exact',1,'measured','unknown',NULL,'unknown'",
        "'scan','complete','r',1,'exact',NULL,'measured','exact',1,'measured','unknown',NULL,'unknown'",
        "'scan','complete','r',1,'exact',1,'measured','lower_bound',NULL,'measured','unknown',NULL,'unknown'",
        "'scan','complete','r',1,'exact',1,'measured','exact',1,'measured','unknown',7,'unknown'",
        "'scan','complete','r',1,'exact',-1,'measured','exact',1,'measured','unknown',NULL,'unknown'",
        "'other','complete','r',1,'exact',1,'measured','exact',1,'measured','unknown',NULL,'unknown'",
        "'scan','done','r',1,'exact',1,'measured','exact',1,'measured','unknown',NULL,'unknown'",
        "'scan','complete','r',0,'exact',1,'measured','exact',1,'measured','unknown',NULL,'unknown'",
    ] {
        assert!(insert(bad).is_err(), "{bad}");
    }
    Ok(())
}

#[test]
fn rows_cannot_be_updated_and_are_deleted_only_through_the_open_gate() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    db.append_scope_observation(&observation(1, 1, 1_000))
        .map_err(|e| format!("{e:?}"))?;
    let update = db.connection.execute(
        "UPDATE analysis_index_scope_observation SET logical_bytes = 1",
        [],
    );
    assert!(update.is_err_and(|e| e.to_string().contains("append_only")));
    let delete = db
        .connection
        .execute("DELETE FROM analysis_index_scope_observation", []);
    assert!(delete.is_err_and(|e| e.to_string().contains("append_only")));
    assert_eq!(count(&db)?, 1);
    db.connection.execute_batch(
        "BEGIN IMMEDIATE;
         UPDATE app_db_purge_gate SET open = 1;
         DELETE FROM analysis_index_scope_observation;
         UPDATE app_db_purge_gate SET open = 0;
         COMMIT;",
    )?;
    assert_eq!(count(&db)?, 0);
    Ok(())
}

#[test]
fn a_busy_database_is_retried_only_a_bounded_number_of_times() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let other =
        AppDb::open_gui_with_history(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    db.connection.busy_timeout(Duration::from_millis(30))?;
    other.connection.execute_batch("BEGIN IMMEDIATE")?;
    let started = Instant::now();
    let row = observation(1, 1, 1_000);
    assert_eq!(
        db.append_scope_observation(&row),
        Err(ScopeObservationWriteError::Busy)
    );
    // Three attempts each waited for the busy timeout, and no fourth was made.
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(80), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    other.connection.execute_batch("ROLLBACK")?;
    assert_eq!(count(&other)?, 0);
    assert_eq!(
        db.append_scope_observation(&row),
        Ok(ScopeObservationAppend::Added)
    );
    assert_eq!(count(&other)?, 1);
    Ok(())
}

#[test]
fn a_lock_released_during_the_attempts_lets_the_append_succeed_once() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let other =
        AppDb::open_gui_with_history(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    db.connection.busy_timeout(Duration::from_millis(25))?;
    other.connection.execute_batch("BEGIN IMMEDIATE")?;
    let holder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        other.connection.execute_batch("ROLLBACK")
    });
    let row = observation(1, 1, 1_000);
    let result = db.append_scope_observation_with(&row, Retention::DEFAULT, 40);
    holder.join().map_err(|_| "lock holder panicked")??;
    assert_eq!(result, Ok(ScopeObservationAppend::Added));
    assert_eq!(count(&db)?, 1);
    Ok(())
}
#[test]
fn a_full_filesystem_rolls_the_whole_append_back() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    db.append_scope_observation(&observation(1, 1, 1_000))
        .map_err(|e| format!("{e:?}"))?;
    let pages: i64 = db
        .connection
        .pragma_query_value(None, "page_count", |row| row.get(0))?;
    db.connection.pragma_update(None, "max_page_count", pages)?;
    let mut large = observation(2, 1, 2_000);
    large.size_rule_id = "x".repeat(2_000_000);
    assert_eq!(
        db.append_scope_observation(&large),
        Err(ScopeObservationWriteError::DiskFull)
    );
    assert_eq!(count(&db)?, 1);
    Ok(())
}

#[test]
fn rows_survive_closing_and_reopening_the_database() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let mut db = installed(&dir)?;
    let row = observation(1, 1, 1_000);
    db.append_scope_observation(&row)
        .map_err(|e| format!("{e:?}"))?;
    db.close().map_err(|e| format!("{e:?}"))?;
    let mut reopened = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        reopened.append_scope_observation(&row),
        Ok(ScopeObservationAppend::AlreadyRecorded)
    );
    let listed = reopened
        .list(&fingerprint(1), 1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(listed.rows, [row]);
    Ok(())
}

/// A database migrated through the production steps up to and including `version`.
pub(super) fn database_at(
    dir: &Path,
    version: u32,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let path = dir.join(format!("v{version}.db"));
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    for definition in HISTORY_DEFINITIONS
        .steps
        .iter()
        .take_while(|definition| definition.number <= version)
    {
        let backup = dir.join(format!("before-{version}-{}.db", definition.number));
        migration::apply_next(&path, &backup, &HISTORY_DEFINITIONS)
            .map_err(|e| format!("{e:?}"))?;
    }
    Ok(path)
}
