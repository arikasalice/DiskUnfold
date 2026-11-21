use super::*;
use rusqlite::params;
use std::{collections::BTreeSet, time::Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn writer(path: &std::path::Path) -> Result<AppDb, std::io::Error> {
    AppDb::create_gui_with_history(path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))
}

fn id(seed: u8) -> Id {
    let mut bytes = [seed; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    Id::from_v7_bytes(bytes)
}

fn insert_history(db: &mut AppDb, job: &Id, reckon: i64, seq: i64, item_rows: u64) -> TestResult {
    let bytes = job.v7_bytes().ok_or("invalid job id")?;
    db.connection.execute(
        "INSERT INTO job_runtime_job_link
         (job_id, record_origin, reckon_at_ms, record_seq, created_at_ms)
         VALUES (?1, 'this_app', ?2, ?3, ?4)",
        params![bytes.as_slice(), reckon, seq, reckon],
    )?;
    db.connection.execute(
        "INSERT INTO job_runtime_job_record
         (job_id, kind_id, kind_version, scope_summary, final_state,
          completed_count, overwritten_count, failed_count, skipped_count,
          unprocessed_count, started_at_ms, finished_at_ms, correlation_id, created_at_ms)
         VALUES (?1, 'scan.metadata', 1, 'metadata: 1 root', 'done', 0, 0, 0, 0, 0, ?2, ?2, ?1, ?2)",
        params![bytes.as_slice(), reckon],
    )?;
    db.connection.execute(
        "INSERT INTO job_runtime_job_scope_root (job_id, root_seq, location_text, created_at_ms)
         VALUES (?1, 0, 'C:/root', ?2)",
        params![bytes.as_slice(), reckon],
    )?;
    for item_seq in 0..item_rows {
        db.connection.execute(
            "INSERT INTO job_runtime_item_result
             (job_id, item_seq, item_ref, location_raw, location_text, outcome,
              review_needed, held, outcome_unknown, observed_size_state, created_at_ms)
             VALUES (?1, ?2, x'01', x'', 'C:/root/item', 'completed', 0, 0, 0, 'unknown', ?3)",
            params![bytes.as_slice(), i64::try_from(item_seq)?, reckon],
        )?;
    }
    db.connection.execute(
        "UPDATE job_runtime_history_state SET last_record_seq = max(last_record_seq, ?1)
         WHERE state_key = 'history'",
        [seq],
    )?;
    Ok(())
}

fn candidates(db: &mut AppDb, limit: usize) -> Result<Vec<HistoryPruneCandidate>, Error> {
    db.read_history_prune_candidates(0, 100, None, limit)
}

#[test]
fn schema_has_purge_gate_and_generation_before_batch_deletion() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let mut db = writer(&path)?;
    let gate: bool = db.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='app_db_purge_gate')",
        [],
        |row| row.get(0),
    )?;
    let generation: bool = db.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('job_runtime_history_state') WHERE name='delete_generation')",
        [],
        |row| row.get(0),
    )?;
    assert!(gate);
    assert!(generation);

    let job = id(1);
    insert_history(&mut db, &job, 10, 1, 0)?;
    let selected = candidates(&mut db, 10)?;
    db.delete_expired_history_batch(&selected, 0, 100, 101)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

#[test]
fn delete_list_covers_every_job_runtime_table_with_job_id() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = writer(&dir.path().join("history.db"))?;
    let mut actual = BTreeSet::new();
    let mut tables = db.connection.prepare(
        "SELECT name FROM sqlite_schema WHERE type='table' AND name LIKE 'job_runtime_%' ORDER BY name",
    )?;
    let names = tables.query_map([], |row| row.get::<_, String>(0))?;
    for name in names {
        let name = name?;
        let has_job_id: bool = db.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name='job_id')",
            [&name],
            |row| row.get::<_, bool>(0),
        )?;
        if has_job_id {
            actual.insert(name);
        }
    }
    assert_eq!(
        actual,
        JOB_ID_TABLES
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    );
    assert!(actual.contains("job_runtime_job_scope_root"));
    Ok(())
}

#[test]
fn prune_candidates_are_keyset_paged_in_reckon_order_with_row_counts() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = writer(&dir.path().join("history.db"))?;
    let later = id(2);
    let earlier = id(1);
    insert_history(&mut db, &later, 20, 2, 0)?;
    insert_history(&mut db, &earlier, 10, 1, 2)?;

    let first = db.read_history_prune_candidates(0, 100, None, 1)?;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].job_id, earlier);
    assert_eq!(first[0].row_count, 5);
    let next = db.read_history_prune_candidates(0, 100, Some(first[0].position), 1)?;
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].job_id, later);
    assert_eq!(next[0].row_count, 3);
    Ok(())
}

#[test]
fn delete_commits_all_job_rows_and_increments_generation_once() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = writer(&dir.path().join("history.db"))?;
    let target = id(3);
    let other = id(4);
    insert_history(&mut db, &target, 10, 1, 2)?;
    insert_history(&mut db, &other, 20, 2, 1)?;
    db.connection.execute(
        "INSERT INTO job_runtime_approval_check
         (check_id, job_id, request_version_id, request_kind, version_hash,
          hash_format, operation_kind, record_kind, check_result, approved_at_ms,
          created_at_ms, correlation_id)
         VALUES (?1, ?2, ?3, 'plan', zeroblob(32), 'du.plan.hash.v1', 'trash',
                 'start', 'matched', 10, 10, ?4)",
        params![
            id(21).v7_bytes().ok_or("invalid check id")?.as_slice(),
            target.v7_bytes().ok_or("invalid target")?.as_slice(),
            id(22)
                .v7_bytes()
                .ok_or("invalid request version")?
                .as_slice(),
            id(23).v7_bytes().ok_or("invalid correlation")?.as_slice(),
        ],
    )?;
    let selected = candidates(&mut db, 10)?;
    let candidate = selected
        .iter()
        .find(|candidate| candidate.job_id == target)
        .cloned()
        .ok_or("target missing")?;

    let receipt = db
        .delete_expired_history_batch(&[candidate], 0, 100, 101)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        receipt.deleted_job_ids.as_slice(),
        std::slice::from_ref(&target)
    );
    assert_eq!(receipt.deleted_row_count, 6);
    assert_eq!(receipt.revision.delete_generation, 1);
    for table in JOB_ID_TABLES {
        let remaining: i64 = db.connection.query_row(
            &format!("SELECT count(*) FROM {table} WHERE job_id=?1"),
            [target.v7_bytes().ok_or("invalid job id")?.as_slice()],
            |row| row.get(0),
        )?;
        assert_eq!(remaining, 0, "{table}");
    }
    let other_rows: i64 = db.connection.query_row(
        "SELECT count(*) FROM job_runtime_job_record WHERE job_id=?1",
        [other.v7_bytes().ok_or("invalid job id")?.as_slice()],
        |row| row.get(0),
    )?;
    assert_eq!(other_rows, 1);
    let gate: i64 = db.connection.query_row(
        "SELECT open FROM app_db_purge_gate WHERE gate_key=1",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(gate, 0);
    Ok(())
}

#[test]
fn failed_table_delete_rolls_back_rows_gate_and_generation() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = writer(&dir.path().join("history.db"))?;
    let target = id(5);
    insert_history(&mut db, &target, 10, 1, 2)?;
    let selected = candidates(&mut db, 10)?;
    let rows_before = count_history_rows(&db, &target)?;
    db.connection.execute_batch(
        "CREATE TRIGGER injected_prune_failure BEFORE DELETE ON job_runtime_job_record
         BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    )?;

    assert_eq!(
        db.delete_expired_history_batch(&selected, 0, 100, 101),
        Err(HistoryPruneDeleteError::Unavailable)
    );
    assert_eq!(count_history_rows(&db, &target)?, rows_before);
    let (gate, generation): (i64, i64) = db.connection.query_row(
        "SELECT (SELECT open FROM app_db_purge_gate), delete_generation
         FROM job_runtime_history_state WHERE state_key='history'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(gate, 0);
    assert_eq!(generation, 0);
    Ok(())
}

fn count_history_rows(db: &AppDb, job: &Id) -> Result<i64, Error> {
    let mut count = 0_i64;
    for table in JOB_ID_TABLES {
        count += db.connection.query_row(
            &format!("SELECT count(*) FROM {table} WHERE job_id=?1"),
            [job.v7_bytes().ok_or(Error::InvalidQuery)?.as_slice()],
            |row| row.get::<_, i64>(0),
        )?;
    }
    Ok(count)
}

#[test]
fn live_successor_turns_a_candidate_into_a_hold_inside_the_delete_transaction() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = writer(&dir.path().join("history.db"))?;
    let parent = id(6);
    let child = id(7);
    insert_history(&mut db, &parent, 10, 1, 0)?;
    let mut selected = candidates(&mut db, 10)?;
    db.connection.execute(
        "INSERT INTO job_runtime_job_link
         (job_id, predecessor_job_id, relation_kind, record_origin, reckon_at_ms, record_seq, created_at_ms)
         VALUES (?1, ?2, 'retry', 'this_app', 20, 2, 20)",
        params![child.v7_bytes().ok_or("invalid child")?.as_slice(), parent.v7_bytes().ok_or("invalid parent")?.as_slice()],
    )?;

    let receipt = db
        .delete_expired_history_batch(&selected, 0, 100, 101)
        .map_err(|error| format!("{error:?}"))?;
    assert!(receipt.deleted_job_ids.is_empty());
    assert_eq!(receipt.revision.delete_generation, 0);
    let parent_exists: bool = db.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM job_runtime_job_record WHERE job_id=?1)",
        [parent.v7_bytes().ok_or("invalid parent")?.as_slice()],
        |row| row.get(0),
    )?;
    assert!(parent_exists);
    selected.clear();
    Ok(())
}

#[test]
fn pruning_parent_leaves_terminal_follow_up_and_plan_version_reference_intact() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = writer(&dir.path().join("history.db"))?;
    let parent = id(11);
    let child = id(12);
    let plan_version = id(13);
    insert_history(&mut db, &parent, 10, 1, 0)?;
    db.connection.execute(
        "INSERT INTO job_runtime_job_link
         (job_id, plan_version_id, predecessor_job_id, relation_kind, record_origin,
          reckon_at_ms, record_seq, created_at_ms)
         VALUES (?1, ?2, ?3, 'retry', 'this_app', 20, 2, 20)",
        params![
            child.v7_bytes().ok_or("invalid child")?.as_slice(),
            plan_version
                .v7_bytes()
                .ok_or("invalid plan version")?
                .as_slice(),
            parent.v7_bytes().ok_or("invalid parent")?.as_slice(),
        ],
    )?;
    db.connection.execute(
        "INSERT INTO job_runtime_job_record
         (job_id, kind_id, kind_version, scope_summary, final_state,
          completed_count, overwritten_count, failed_count, skipped_count,
          unprocessed_count, started_at_ms, finished_at_ms, correlation_id, created_at_ms)
         VALUES (?1, 'scan.metadata', 1, 'metadata: 1 root', 'done', 0, 0, 0, 0, 0, 20, 20, ?1, 20)",
        [child.v7_bytes().ok_or("invalid child")?.as_slice()],
    )?;
    db.connection.execute(
        "UPDATE job_runtime_history_state SET last_record_seq=2 WHERE state_key='history'",
        [],
    )?;
    let parent_candidate = candidates(&mut db, 10)?
        .into_iter()
        .find(|candidate| candidate.job_id == parent)
        .ok_or("parent missing")?;
    assert!(!parent_candidate.has_nonterminal_follow_up);

    let receipt = db
        .delete_expired_history_batch(&[parent_candidate], 0, 100, 101)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(receipt.deleted_job_ids, [parent]);
    let (child_link, child_record, stored_plan): (i64, i64, Vec<u8>) = db.connection.query_row(
        "SELECT
            EXISTS(SELECT 1 FROM job_runtime_job_link WHERE job_id=?1),
            EXISTS(SELECT 1 FROM job_runtime_job_record WHERE job_id=?1),
            plan_version_id
         FROM job_runtime_job_link WHERE job_id=?1",
        [child.v7_bytes().ok_or("invalid child")?.as_slice()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!((child_link, child_record), (1, 1));
    assert_eq!(
        stored_plan,
        plan_version.v7_bytes().ok_or("invalid plan version")?
    );
    Ok(())
}

#[test]
fn reopening_after_a_committed_batch_keeps_integrity_and_only_whole_jobs() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let first = id(14);
    let second = id(15);
    {
        let mut db = writer(&path)?;
        insert_history(&mut db, &first, 10, 1, 1)?;
        insert_history(&mut db, &second, 20, 2, 2)?;
        let first_candidate = candidates(&mut db, 10)?
            .into_iter()
            .find(|candidate| candidate.job_id == first)
            .ok_or("first batch job missing")?;
        db.delete_expired_history_batch(&[first_candidate], 0, 100, 101)
            .map_err(|error| format!("{error:?}"))?;
    }

    let reopened = AppDb::open_read_only_live(&path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let integrity: String = reopened
        .connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    assert_eq!(integrity, "ok");
    let (first_rows, second_rows): (i64, i64) = reopened.connection.query_row(
        "SELECT
            (SELECT count(*) FROM job_runtime_job_link WHERE job_id=?1),
            (SELECT count(*) FROM job_runtime_job_link WHERE job_id=?2)",
        params![
            first.v7_bytes().ok_or("invalid first")?.as_slice(),
            second.v7_bytes().ok_or("invalid second")?.as_slice(),
        ],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!((first_rows, second_rows), (0, 1));
    Ok(())
}

#[test]
fn prune_query_plans_use_reckon_index_and_job_id_keys() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = writer(&dir.path().join("history.db"))?;
    let select = candidate_query();
    let mut statement = db
        .connection
        .prepare(&format!("EXPLAIN QUERY PLAN {select}"))?;
    let select_plan = statement
        .query_map(
            params![
                0_i64,
                100_i64,
                Option::<i64>::None,
                Option::<i64>::None,
                500_i64
            ],
            |row| row.get::<_, String>(3),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        select_plan
            .iter()
            .any(|line| line.contains("ix_job_runtime_job_link_reckon"))
    );

    let mut delete_plans = Vec::new();
    for table in JOB_ID_TABLES {
        let sql = format!("EXPLAIN QUERY PLAN DELETE FROM {table} WHERE job_id IN (?1, ?2)");
        let plan = db
            .connection
            .prepare(&sql)?
            .query_map(
                params![
                    id(8).v7_bytes().ok_or("invalid id")?.as_slice(),
                    id(9).v7_bytes().ok_or("invalid id")?.as_slice()
                ],
                |row| row.get::<_, String>(3),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        assert!(!plan.is_empty(), "{table} has no delete plan");
        assert!(
            plan.iter()
                .any(|line| line.contains("PRIMARY KEY") || line.contains("job_id")),
            "{table}: {plan:?}"
        );
        delete_plans.push((table, plan));
    }
    println!("history prune selection plan: {select_plan:?}");
    println!("history prune batch delete plans: {delete_plans:?}");
    Ok(())
}

#[test]
#[ignore = "one-million-row SQLite write-lock measurement"]
fn history_prune_measures_one_million_item_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = writer(&dir.path().join("history.db"))?;
    let target = id(10);
    insert_history(&mut db, &target, 10, 1, 0)?;
    {
        let tx = db
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO job_runtime_item_result
                 (job_id, item_seq, item_ref, location_raw, location_text, outcome,
                  review_needed, held, outcome_unknown, observed_size_state, created_at_ms)
                 VALUES (?1, ?2, x'01', x'', 'C:/root/item', 'completed', 0, 0, 0, 'unknown', 10)",
            )?;
            let bytes = target.v7_bytes().ok_or("invalid job id")?;
            for item_seq in 0..1_000_000_i64 {
                insert.execute(params![bytes.as_slice(), item_seq])?;
            }
        }
        tx.commit()?;
    }
    let candidate = candidates(&mut db, 1)?.remove(0);
    let lock_started = Instant::now();
    let receipt = db
        .delete_expired_history_batch(&[candidate], 0, 100, 101)
        .map_err(|error| format!("{error:?}"))?;
    println!(
        "history prune write-lock duration: {:.3}s",
        lock_started.elapsed().as_secs_f64()
    );
    assert_eq!(receipt.deleted_row_count, 1_000_003);
    Ok(())
}
