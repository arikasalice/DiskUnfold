use super::*;
use rusqlite::{ToSql, params};

#[test]
fn list_query_plans_use_the_order_index_for_first_and_cursor_pages() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let tx = db.connection.transaction()?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO snapshot_store_snapshot (
                 snapshot_id, file_name, source_version_id, lineage_id, source_kind,
                 observed_at_ms, created_at_ms, scope_label, condition_summary,
                 completeness, format_version, body_size, body_hash, state,
                 verify_failed_at_ms, updated_at_ms, row_version)
             VALUES (?1, ?2, ?3, ?4, 'local', ?5, 1, X'', '{}', 'complete', 1,
                     64, zeroblob(32), 'active', NULL, 1, 1)",
        )?;
        for value in 1..=10_000u64 {
            let snapshot_id = id(value);
            insert.execute(params![
                snapshot_id
                    .v7_bytes()
                    .ok_or("invalid snapshot id")?
                    .as_slice(),
                format!("{}.dusnap", snapshot_id.as_str()),
                id(value + 20_000)
                    .v7_bytes()
                    .ok_or("invalid source id")?
                    .as_slice(),
                id(90_000)
                    .v7_bytes()
                    .ok_or("invalid lineage id")?
                    .as_slice(),
                (value % 10 != 0).then_some((value % 5) as i64),
            ])?;
        }
    }
    tx.commit()?;
    db.connection.execute_batch("ANALYZE")?;

    let first_sql = list_sql(None);
    let first = explain_plan(&db, &first_sql, &[&1_001i64])?;
    assert_list_plan(&first);

    let known_cursor = SnapshotIndexCursor {
        list_seq: 0,
        observed_at_ms: Some(1),
        snapshot_id: id(5_001),
    };
    let known_sql = list_sql(Some(&known_cursor));
    let known = explain_plan(
        &db,
        &known_sql,
        &[
            &1i64,
            &known_cursor
                .snapshot_id
                .v7_bytes()
                .ok_or("invalid cursor id")?
                .as_slice(),
            &1_001i64,
        ],
    )?;
    assert_list_plan(&known);

    let older = explain_plan(&db, &list_known_older_sql(), &[&1i64, &1_001i64])?;
    assert_list_plan(&older);

    let null_tail = explain_plan(&db, &list_null_tail_sql(), &[&1_001i64])?;
    assert_list_plan(&null_tail);

    let unknown_cursor = SnapshotIndexCursor {
        list_seq: 0,
        observed_at_ms: None,
        snapshot_id: id(10_000),
    };
    let unknown_sql = list_sql(Some(&unknown_cursor));
    let unknown = explain_plan(
        &db,
        &unknown_sql,
        &[
            &unknown_cursor
                .snapshot_id
                .v7_bytes()
                .ok_or("invalid cursor id")?
                .as_slice(),
            &1_001i64,
        ],
    )?;
    assert_list_plan(&unknown);

    println!("SNAPSHOT_INDEX_PLAN_ROWS=10000 NULL_TIMES=1000");
    println!("SNAPSHOT_INDEX_PLAN_FIRST={}", first.join(" | "));
    println!("SNAPSHOT_INDEX_PLAN_CURSOR_KNOWN={}", known.join(" | "));
    println!("SNAPSHOT_INDEX_PLAN_CURSOR_OLDER={}", older.join(" | "));
    println!("SNAPSHOT_INDEX_PLAN_NULL_TAIL={}", null_tail.join(" | "));
    println!("SNAPSHOT_INDEX_PLAN_CURSOR_UNKNOWN={}", unknown.join(" | "));
    Ok(())
}

fn explain_plan(db: &AppDb, sql: &str, parameters: &[&dyn ToSql]) -> rusqlite::Result<Vec<String>> {
    let mut statement = db
        .connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
    statement
        .query_map(parameters, |row| row.get::<_, String>(3))?
        .collect()
}

fn assert_list_plan(plan: &[String]) {
    let text = plan.join(" | ");
    assert!(
        text.contains("ix_snapshot_store_snapshot_list_order"),
        "{text}"
    );
    assert!(!text.contains("TEMP B-TREE"), "{text}");
    assert!(
        !plan
            .iter()
            .any(|step| *step == "SCAN s" || step.starts_with("SCAN s ")),
        "{text}"
    );
}
