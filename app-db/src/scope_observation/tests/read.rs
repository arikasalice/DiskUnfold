//! The read port: order, interval, limit and the older-schema answer.

use super::*;
use du_core_contracts::scope_observation_log::{
    SCOPE_OBSERVATION_LIST_LIMIT, ScopeObservationReadError,
};

fn list_times(
    db: &AppDb,
    from: Option<i64>,
    to: Option<i64>,
    limit: usize,
) -> Result<(Vec<i64>, bool), Box<dyn std::error::Error>> {
    let listed = db
        .list(&fingerprint(1), 1, from, to, limit)
        .map_err(|e| format!("{e:?}"))?;
    Ok((
        listed.rows.iter().map(|row| row.observed_at_ms).collect(),
        listed.truncated,
    ))
}

fn seeded(dir: &tempfile::TempDir, times: &[i64]) -> Result<AppDb, Box<dyn std::error::Error>> {
    let mut db = installed(dir)?;
    for (job, at) in times.iter().enumerate() {
        db.append_scope_observation(&observation(job as u64 + 1, 1, *at))
            .map_err(|e| format!("{e:?}"))?;
    }
    Ok(db)
}

#[test]
fn rows_come_back_by_time_then_identifier_whatever_the_insert_order() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    for (job, at) in [(3, 200), (1, 200), (5, 100), (2, 200), (4, 300)] {
        db.append_scope_observation(&observation(job, 1, at))
            .map_err(|e| format!("{e:?}"))?;
    }
    let listed = db
        .list(&fingerprint(1), 1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    let order: Vec<(i64, Id)> = listed
        .rows
        .iter()
        .map(|row| (row.observed_at_ms, row.job_id.clone()))
        .collect();
    assert_eq!(
        order,
        [
            (100, id(5)),
            (200, id(1)),
            (200, id(2)),
            (200, id(3)),
            (300, id(4))
        ]
    );
    Ok(())
}

#[test]
fn the_interval_is_inclusive_and_either_end_may_be_open() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = seeded(&dir, &[100, 200, 300, 400])?;
    assert_eq!(list_times(&db, Some(200), Some(300), 10)?.0, [200, 300]);
    assert_eq!(
        list_times(&db, Some(201), Some(299), 10)?.0,
        Vec::<i64>::new()
    );
    assert_eq!(list_times(&db, Some(300), None, 10)?.0, [300, 400]);
    assert_eq!(list_times(&db, None, Some(200), 10)?.0, [100, 200]);
    assert_eq!(
        list_times(&db, Some(i64::MIN), Some(i64::MAX), 10)?.0.len(),
        4
    );
    Ok(())
}

#[test]
fn the_limit_returns_the_earliest_rows_and_says_when_it_cut_the_answer() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = seeded(&dir, &[100, 200, 300, 400, 500])?;
    assert_eq!(list_times(&db, None, None, 2)?, (vec![100, 200], true));
    assert_eq!(
        list_times(&db, None, None, 4)?,
        (vec![100, 200, 300, 400], true)
    );
    assert!(!list_times(&db, None, None, 5)?.1);
    assert!(!list_times(&db, None, None, 6)?.1);
    assert_eq!(
        list_times(&db, Some(400), None, 2)?,
        (vec![400, 500], false)
    );
    Ok(())
}

#[test]
fn the_limit_is_at_most_one_thousand_and_at_least_one() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = installed(&dir)?;
    bulk_insert(&db, 1, SCOPE_OBSERVATION_LIST_LIMIT as i64 + 5, 1_000, 10)?;
    let listed = db
        .list(&fingerprint(1), 1, None, None, SCOPE_OBSERVATION_LIST_LIMIT)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!((listed.rows.len(), listed.truncated), (1_000, true));
    for bad in [0, SCOPE_OBSERVATION_LIST_LIMIT + 1, usize::MAX] {
        assert_eq!(
            db.list(&fingerprint(1), 1, None, None, bad),
            Err(ScopeObservationReadError::InvalidQuery)
        );
    }
    assert_eq!(
        db.list(&fingerprint(1), 0, None, None, 1),
        Err(ScopeObservationReadError::InvalidQuery)
    );
    Ok(())
}

#[test]
fn a_scope_and_a_fingerprint_format_see_only_their_own_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = seeded(&dir, &[100])?;
    db.append_scope_observation(&observation(10, 2, 150))
        .map_err(|e| format!("{e:?}"))?;
    let mut newer_format = observation(11, 1, 160);
    newer_format.fingerprint_format = 2;
    db.append_scope_observation(&newer_format)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(list_times(&db, None, None, 10)?.0, [100]);
    let second = db
        .list(&fingerprint(1), 2, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(second.rows, [newer_format]);
    let missing = db
        .list(&fingerprint(9), 1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert!(missing.rows.is_empty() && !missing.truncated);
    Ok(())
}

#[test]
fn a_row_that_breaks_the_table_contract_is_reported_as_corrupt() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = seeded(&dir, &[100])?;
    db.connection.execute_batch(
        "DROP TRIGGER analysis_index_scope_observation_no_update;
         PRAGMA ignore_check_constraints = ON;
         UPDATE analysis_index_scope_observation SET completeness = 'bogus';",
    )?;
    assert_eq!(
        db.list(&fingerprint(1), 1, None, None, 10),
        Err(ScopeObservationReadError::Corrupt)
    );
    Ok(())
}

#[test]
fn a_database_older_than_the_table_is_unsupported_rather_than_empty() -> TestResult {
    let dir = tempfile::tempdir()?;
    for version in [1, 2, 6] {
        let path = database_at(dir.path(), version)?;
        let reader = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
        assert_eq!(reader.schema_version()?, version);
        assert_eq!(
            reader.list(&fingerprint(1), 1, None, None, 10),
            Err(ScopeObservationReadError::UnsupportedSchema),
            "version {version}"
        );
    }
    Ok(())
}

#[test]
fn a_read_only_connection_lists_rows_and_an_upgraded_database_starts_empty() -> TestResult {
    let dir = tempfile::tempdir()?;
    let upgraded = database_at(dir.path(), 7)?;
    let reader = AppDb::open_read_only(&upgraded).map_err(|e| format!("{e:?}"))?;
    let empty = reader
        .list(&fingerprint(1), 1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert!(empty.rows.is_empty() && !empty.truncated);
    drop(reader);
    let mut writer = AppDb::open_gui_with_history(&upgraded).map_err(|e| format!("{e:?}"))?;
    let row = observation(1, 1, 100);
    writer
        .append_scope_observation(&row)
        .map_err(|e| format!("{e:?}"))?;
    writer.close().map_err(|e| format!("{e:?}"))?;
    let reader = AppDb::open_read_only(&upgraded).map_err(|e| format!("{e:?}"))?;
    let listed = reader
        .list(&fingerprint(1), 1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(listed.rows, [row]);
    Ok(())
}
