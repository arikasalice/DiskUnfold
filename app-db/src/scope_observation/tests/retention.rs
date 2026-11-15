//! Retention: newest 2,000 rows per scope, newest 100,000 overall, one transaction.

use super::*;
use crate::recent_scope::{RecentScopeStore, RecentWrite};
use std::num::NonZeroUsize;

fn gate(db: &AppDb) -> rusqlite::Result<i64> {
    db.connection
        .query_row("SELECT open FROM app_db_purge_gate", [], |row| row.get(0))
}

fn range(db: &AppDb, scope: u16) -> rusqlite::Result<(i64, i64, i64)> {
    db.connection.query_row(
        "SELECT count(*), COALESCE(min(observed_at_ms), 0), COALESCE(max(observed_at_ms), 0)
         FROM analysis_index_scope_observation WHERE scope_fingerprint = ?1",
        [fingerprint(scope).as_slice()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
}

fn append(db: &mut AppDb, job: u64, scope: u16, at: i64) -> TestResult {
    assert_eq!(
        db.append_scope_observation(&observation(job, scope, at)),
        Ok(ScopeObservationAppend::Added)
    );
    Ok(())
}

#[test]
fn the_limits_are_the_published_constants() {
    assert_eq!((PER_SCOPE_LIMIT, TOTAL_LIMIT), (2_000, 100_000));
    assert_eq!(APPEND_ATTEMPTS, 3);
}

#[test]
fn one_scope_keeps_its_newest_two_thousand_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let below = PER_SCOPE_LIMIT as i64 - 2;
    bulk_insert(&db, 1, below, 1_000, 10)?;
    bulk_insert(&db, 2, 5, 1_000, 10)?;
    // One below the limit, exactly at it, and then one over it.
    append(&mut db, 1, 1, 1_000_000)?;
    assert_eq!(
        range(&db, 1)?,
        (PER_SCOPE_LIMIT as i64 - 1, 1_000, 1_000_000)
    );
    append(&mut db, 2, 1, 1_000_010)?;
    assert_eq!(range(&db, 1)?, (PER_SCOPE_LIMIT as i64, 1_000, 1_000_010));
    append(&mut db, 3, 1, 1_000_020)?;
    assert_eq!(range(&db, 1)?, (PER_SCOPE_LIMIT as i64, 1_010, 1_000_020));
    // Another scope's rows are not touched, and the gate is closed again.
    assert_eq!(range(&db, 2)?.0, 5);
    assert_eq!(gate(&db)?, 0);
    Ok(())
}

#[test]
fn a_row_older_than_a_full_scope_is_trimmed_at_once() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    bulk_insert(&db, 1, PER_SCOPE_LIMIT as i64, 10_000, 10)?;
    append(&mut db, 1, 1, 5)?;
    assert_eq!(
        range(&db, 1)?,
        (PER_SCOPE_LIMIT as i64, 10_000, 10_000 + 10 * 1_999)
    );
    let kept: i64 = db.connection.query_row(
        "SELECT count(*) FROM analysis_index_scope_observation WHERE observed_at_ms = 5",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(kept, 0);
    Ok(())
}

#[test]
fn all_scopes_keep_their_newest_hundred_thousand_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    // 50 full scopes of 1,999 rows and one of 49: 99,999 rows, none above its own limit.
    for scope in 0..50u16 {
        bulk_insert(
            &db,
            scope,
            PER_SCOPE_LIMIT as i64 - 1,
            1_000 + i64::from(scope),
            100,
        )?;
    }
    bulk_insert(&db, 100, 49, 5_000_000, 100)?;
    assert_eq!(count(&db)?, TOTAL_LIMIT as i64 - 1);
    let oldest = |db: &AppDb| -> rusqlite::Result<i64> {
        db.connection.query_row(
            "SELECT min(observed_at_ms) FROM analysis_index_scope_observation",
            [],
            |row| row.get(0),
        )
    };
    assert_eq!(oldest(&db)?, 1_000);
    // At the limit: nothing is removed.
    append(&mut db, 1, 101, 9_000_000)?;
    assert_eq!((count(&db)?, oldest(&db)?), (TOTAL_LIMIT as i64, 1_000));
    // Over it: the oldest row of any scope goes, other rows stay.
    append(&mut db, 2, 101, 9_000_100)?;
    assert_eq!(count(&db)?, TOTAL_LIMIT as i64);
    assert_eq!(oldest(&db)?, 1_001);
    assert_eq!(range(&db, 0)?.0, PER_SCOPE_LIMIT as i64 - 2);
    assert_eq!(gate(&db)?, 0);
    Ok(())
}

fn block_gate_close(db: &AppDb) -> rusqlite::Result<()> {
    db.connection.execute_batch(
        "CREATE TRIGGER block_gate_close BEFORE UPDATE ON app_db_purge_gate
         WHEN NEW.open = 0 BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
    )
}

#[test]
fn a_failure_inside_the_trim_rolls_back_the_new_row_and_the_deletions() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let small = Retention {
        per_scope: 3,
        total: 100,
    };
    for (job, at) in [(1, 100), (2, 200), (3, 300)] {
        db.append_scope_observation_with(&observation(job, 1, at), small, 1)
            .map_err(|e| format!("{e:?}"))?;
    }
    let before = db
        .list(&fingerprint(1), 1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    block_gate_close(&db)?;
    assert_eq!(
        db.append_scope_observation_with(&observation(4, 1, 400), small, 1),
        Err(ScopeObservationWriteError::Storage)
    );
    // The row is absent, the deleted row is back and the gate is still closed.
    let after = db
        .list(&fingerprint(1), 1, None, None, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(after, before);
    assert_eq!(gate(&db)?, 0);
    db.connection
        .execute_batch("DROP TRIGGER block_gate_close")?;
    assert_eq!(
        db.append_scope_observation_with(&observation(4, 1, 400), small, 1),
        Ok(ScopeObservationAppend::Added)
    );
    assert_eq!(range(&db, 1)?, (3, 200, 400));
    Ok(())
}

#[test]
fn a_failure_in_the_total_trim_rolls_back_too() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let small = Retention {
        per_scope: 100,
        total: 2,
    };
    for (job, scope, at) in [(1, 1, 100), (2, 2, 200)] {
        db.append_scope_observation_with(&observation(job, scope, at), small, 1)
            .map_err(|e| format!("{e:?}"))?;
    }
    block_gate_close(&db)?;
    assert_eq!(
        db.append_scope_observation_with(&observation(3, 3, 300), small, 1),
        Err(ScopeObservationWriteError::Storage)
    );
    assert_eq!(count(&db)?, 2);
    assert_eq!(range(&db, 1)?.0, 1);
    assert_eq!(range(&db, 3)?.0, 0);
    Ok(())
}

#[test]
fn recent_scope_eviction_does_not_delete_observations() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    append(&mut db, 1, 1, 100)?;
    append(&mut db, 2, 2, 200)?;
    let cap = NonZeroUsize::new(1).ok_or("zero")?;
    let accept = |_: &str| true;
    for (n, scope) in [(1u8, 1u16), (2, 2)] {
        let mut recent_id = [0u8; 16];
        recent_id[6] = 0x70;
        recent_id[8] = 0x80;
        recent_id[15] = n;
        RecentScopeStore::new(&mut db, cap, &accept)
            .map_err(|e| format!("{e:?}"))?
            .put(&RecentWrite {
                id: recent_id,
                fingerprint: fingerprint(scope),
                body: "{}",
                include_count: 1,
                now_ms: i64::from(n),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let recent: i64 =
        db.connection
            .query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| {
                row.get(0)
            })?;
    assert_eq!(recent, 1);
    assert_eq!(count(&db)?, 2);
    Ok(())
}
