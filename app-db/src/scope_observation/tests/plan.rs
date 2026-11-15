//! Query plans and work bounds of the list and retention statements.

use super::*;
use crate::scope_observation::decode::LIST_SQL;
use rusqlite::{StatementStatus, params};

const INDEX: &str = "ix_analysis_index_scope_observation_scope";

fn plan(db: &AppDb, sql: &str, params: &[&dyn rusqlite::ToSql]) -> rusqlite::Result<Vec<String>> {
    let mut statement = db
        .connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
    let rows = statement.query_map(params, |row| row.get::<_, String>(3))?;
    rows.collect()
}

fn assert_indexed(plan: &[String]) {
    let text = plan.join(" | ");
    assert!(text.contains(INDEX), "{text}");
    assert!(!text.contains("TEMP B-TREE"), "{text}");
    assert!(
        !text.starts_with("SCAN analysis_index_scope_observation |"),
        "{text}"
    );
}

#[test]
fn list_and_per_scope_retention_use_the_scope_index_without_sorting() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = installed(&dir)?;
    for scope in 0..20 {
        bulk_insert(&db, scope, 150, 1_000, 10)?;
    }
    let fp = fingerprint(7);
    let (from, to, limit) = (i64::MIN, i64::MAX, 1_001i64);
    assert_indexed(&plan(
        &db,
        LIST_SQL,
        &[&fp.as_slice(), &1i64, &from, &to, &limit],
    )?);
    assert_indexed(&plan(
        &db,
        CUTOFF_SQL,
        &[&fp.as_slice(), &1i64, &(PER_SCOPE_LIMIT as i64)],
    )?);
    let id = [0u8; 16];
    assert_indexed(&plan(
        &db,
        DELETE_SCOPE_SQL,
        &[&fp.as_slice(), &1i64, &5_000i64, &id.as_slice()],
    )?);
    // The total limit walks one index to count, and sorts only when it must delete.
    let counted = plan(&db, COUNT_SQL, &[])?.join(" | ");
    assert!(counted.contains("COVERING INDEX"), "{counted}");
    Ok(())
}

fn steps(statement: &rusqlite::Statement<'_>) -> (i32, i32, i32) {
    (
        statement.get_status(StatementStatus::VmStep),
        statement.get_status(StatementStatus::FullscanStep),
        statement.get_status(StatementStatus::Sort),
    )
}

/// 200 scopes of 2,000 rows each. Run with
/// `cargo test -p du-app-db plan_and_work -- --ignored --nocapture`.
#[test]
#[ignore = "builds 400,000 rows; run explicitly to print the measured work"]
fn plan_and_work_at_200_scopes_by_2000_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = installed(&dir)?;
    for scope in 0..200 {
        bulk_insert(&db, scope, 2_000, 1_000, 10)?;
    }
    bulk_insert(&db, 100, 1, 1_000_000, 10)?;
    assert_eq!(count(&db)?, 400_001);
    let fp = fingerprint(100);
    for limit in [10i64, 1_000] {
        let started = Instant::now();
        let mut statement = db.connection.prepare(LIST_SQL)?;
        let rows = statement
            .query_map(
                params![fp.as_slice(), 1, i64::MIN, i64::MAX, limit + 1],
                |_| Ok(()),
            )?
            .count();
        let (vm, full, sort) = steps(&statement);
        eprintln!(
            "list limit {limit}: rows {rows}, vm steps {vm}, full-scan steps {full}, sorts {sort}, {:?}",
            started.elapsed()
        );
        assert_eq!((full, sort), (0, 0));
    }
    let started = Instant::now();
    let mut cutoff = db.connection.prepare(CUTOFF_SQL)?;
    let found: (i64, Vec<u8>) = cutoff
        .query_row(params![fp.as_slice(), 1, PER_SCOPE_LIMIT as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    let (vm, full, sort) = steps(&cutoff);
    eprintln!(
        "cutoff: vm steps {vm}, full-scan steps {full}, sorts {sort}, {:?}",
        started.elapsed()
    );
    assert_eq!((full, sort), (0, 0));
    db.connection
        .execute_batch("BEGIN; UPDATE app_db_purge_gate SET open = 1;")?;
    let started = Instant::now();
    let mut delete = db.connection.prepare(DELETE_SCOPE_SQL)?;
    let removed = delete.execute(params![fp.as_slice(), 1, found.0, found.1])?;
    let (vm, full, sort) = steps(&delete);
    eprintln!(
        "per-scope delete: rows deleted {removed}, vm steps {vm}, full-scan steps {full}, sorts {sort}, {:?}",
        started.elapsed()
    );
    assert_eq!((removed, full, sort), (1, 0, 0));
    drop(delete);
    db.connection.execute_batch("ROLLBACK")?;

    // The total step on a table at its limit: one count, then one sorted delete.
    let dir = tempfile::tempdir()?;
    let db = installed(&dir)?;
    for scope in 0..50 {
        bulk_insert(&db, scope, 2_000, 1_000 + i64::from(scope), 100)?;
    }
    bulk_insert(&db, 99, 1, 9_000_000, 10)?;
    assert_eq!(count(&db)?, TOTAL_LIMIT as i64 + 1);
    let started = Instant::now();
    let mut counting = db.connection.prepare(COUNT_SQL)?;
    let counted: i64 = counting.query_row([], |row| row.get(0))?;
    let (vm, full, sort) = steps(&counting);
    eprintln!(
        "count at {counted}: vm steps {vm}, full-scan steps {full}, sorts {sort}, {:?}",
        started.elapsed()
    );
    db.connection
        .execute_batch("BEGIN; UPDATE app_db_purge_gate SET open = 1;")?;
    let started = Instant::now();
    let mut delete = db.connection.prepare(DELETE_TOTAL_SQL)?;
    let removed = delete.execute([1])?;
    let (vm, full, sort) = steps(&delete);
    eprintln!(
        "total delete of {removed}: vm steps {vm}, full-scan steps {full}, sorts {sort}, {:?}",
        started.elapsed()
    );
    drop(delete);
    db.connection.execute_batch("ROLLBACK")?;
    Ok(())
}
