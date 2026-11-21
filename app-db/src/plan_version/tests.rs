use super::*;
use crate::{AppDb, HISTORY_SCHEMA_VERSION};
use du_core_contracts::{
    Id,
    data_source::DataSource,
    plan_version_store::{PlanOriginChannel, PlanReadiness, PlanVersionRecord, PlanVersionStore},
};
use rusqlite::params;
use std::{
    io::{Error, ErrorKind},
    sync::{Arc, Barrier},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn id(seed: u8) -> Id {
    let mut bytes = [0; 16];
    bytes[0] = seed;
    Id::from_v7_bytes(bytes)
}

fn blob(value: &Id) -> Result<Vec<u8>, Error> {
    value
        .v7_bytes()
        .map(|bytes| bytes.to_vec())
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "invalid test UUID"))
}

fn record(seed: u8, lineage: u8, parent: Option<Id>, created_at_ms: i64) -> PlanVersionRecord {
    PlanVersionRecord {
        plan_version_id: id(seed),
        lineage_id: id(lineage),
        parent_plan_version_id: parent,
        version_hash: [seed; 32],
        hash_format: "du.plan.hash.v1".into(),
        ruleset_version: 1,
        readiness: PlanReadiness::Ready,
        origin_channel: PlanOriginChannel::Gui,
        body: "{}".into(),
        created_at_ms,
        data_source: DataSource::Real,
    }
}

fn database(path: &std::path::Path) -> Result<AppDb, Box<dyn std::error::Error>> {
    AppDb::create_gui_with_history(path).map_err(|error| format!("{error:?}").into())
}

fn add_ruleset(db: &AppDb) -> rusqlite::Result<()> {
    db.connection.execute(
        "INSERT INTO change_planner_ruleset
         (version, ruleset_hash, hash_format, class_table_version,
          known_location_table_version, rule_format_version, origin,
          change_summary, body, created_at_ms)
         VALUES (1, ?1, 'du.ruleset.hash.v1', 'class.1', 'location.1',
                 1, 'gui', '{}', '{}', 1)",
        [vec![0u8; 32]],
    )?;
    Ok(())
}

#[test]
fn append_read_and_lineage_order_use_the_documented_index() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = database(&dir.path().join("plans.db"))?;
    add_ruleset(&db)?;

    let first = record(1, 10, None, 100);
    let second = record(2, 10, Some(first.plan_version_id.clone()), 200);
    let tied = record(3, 10, Some(first.plan_version_id.clone()), 200);
    db.append_fixed(&first)
        .map_err(|error| format!("{error:?}"))?;
    db.append_fixed(&second)
        .map_err(|error| format!("{error:?}"))?;
    db.append_fixed(&tied)
        .map_err(|error| format!("{error:?}"))?;

    assert_eq!(
        db.read(&first.plan_version_id)
            .map_err(|error| format!("{error:?}"))?,
        first
    );
    let id_plan: String = db.connection.query_row(
        "EXPLAIN QUERY PLAN
         SELECT plan_version_id FROM change_planner_plan_version
         WHERE plan_version_id = ?1",
        [blob(&id(1))?],
        |row| row.get(3),
    )?;
    assert!(
        id_plan.contains("sqlite_autoindex_change_planner_plan_version_1"),
        "{id_plan}"
    );
    let lineage = db
        .read_lineage(&id(10))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(lineage, [first, second, tied]);
    let heads = db
        .read_lineage_heads(&id(10))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(heads.len(), 3);
    assert_eq!(heads[0].plan_version_id, id(1));
    assert_eq!(heads[1].plan_version_id, id(2));
    assert_eq!(
        db.read_body(&id(2)).map_err(|error| format!("{error:?}"))?,
        "{}"
    );
    let key = blob(&id(10))?;
    let plan: String = db.connection.query_row(
        "EXPLAIN QUERY PLAN
         SELECT plan_version_id FROM change_planner_plan_version
         WHERE lineage_id = ?1 ORDER BY created_at_ms, plan_version_id",
        [key],
        |row| row.get(3),
    )?;
    assert!(
        plan.contains("ix_change_planner_plan_version_lineage"),
        "{plan}"
    );
    assert!(!plan.contains("USE TEMP B-TREE"), "{plan}");

    let absent = db.read(&id(99)).err().ok_or("missing plan was returned")?;
    assert_eq!(absent.kind, PortErrorKind::NotFound);
    assert_eq!(db.schema_version()?, HISTORY_SCHEMA_VERSION);
    Ok(())
}

#[test]
fn concurrent_fixed_versions_are_serialized_and_both_preserved() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("concurrent.db");
    let db = database(&path)?;
    add_ruleset(&db)?;
    drop(db);

    let left_db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let right_db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let barrier = Arc::new(Barrier::new(3));
    let left_barrier = Arc::clone(&barrier);
    let right_barrier = Arc::clone(&barrier);
    let left_record = record(11, 20, None, 300);
    let right_record = record(12, 20, None, 300);

    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(move || {
            let mut db = left_db;
            left_barrier.wait();
            db.append_fixed(&left_record)
        });
        let right = scope.spawn(move || {
            let mut db = right_db;
            right_barrier.wait();
            db.append_fixed(&right_record)
        });
        barrier.wait();
        (left.join(), right.join())
    });
    assert!(left.is_ok_and(|result| result.is_ok()));
    assert!(right.is_ok_and(|result| result.is_ok()));

    let db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        db.read_lineage(&id(20))
            .map_err(|error| format!("{error:?}"))?
            .len(),
        2
    );
    Ok(())
}

#[test]
fn sample_versions_are_rejected_before_persistence() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = database(&dir.path().join("sample.db"))?;
    add_ruleset(&db)?;
    let mut sample = record(3, 30, None, 100);
    sample.data_source = DataSource::Sample;

    let error = db
        .append_fixed(&sample)
        .err()
        .ok_or("sample plan was stored")?;
    assert_eq!(error.reason_key, "plan.sample_not_stored");
    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM change_planner_plan_version",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
fn caller_input_violations_are_not_reported_as_database_corruption() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = database(&dir.path().join("invalid-input.db"))?;
    add_ruleset(&db)?;
    assert_eq!(MAX_PLAN_BODY_BYTES, 128 * 1024 * 1024);

    let mut empty_hash_format = record(5, 40, None, 100);
    empty_hash_format.hash_format.clear();
    let mut zero_ruleset = record(6, 40, None, 100);
    zero_ruleset.ruleset_version = 0;
    let mut oversized_body = record(7, 40, None, 100);
    oversized_body.body = format!("{}{{}}", " ".repeat(MAX_PLAN_BODY_BYTES - 1));

    for bad in [empty_hash_format, zero_ruleset, oversized_body] {
        let error = db
            .append_fixed(&bad)
            .err()
            .ok_or("invalid plan version was stored")?;
        assert_eq!(error.kind, PortErrorKind::Unsupported);
        assert_eq!(error.reason_key, "db.invalid_input");
    }
    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM change_planner_plan_version",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
fn invalid_json_is_rejected_as_invalid_input() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = database(&dir.path().join("invalid-json.db"))?;
    add_ruleset(&db)?;
    let mut invalid = record(4, 40, None, 100);
    invalid.body = "{".into();

    let error = db
        .append_fixed(&invalid)
        .err()
        .ok_or("invalid JSON plan was stored")?;
    assert_eq!(error.kind, PortErrorKind::Unsupported);
    assert_eq!(error.reason_key, "db.invalid_input");
    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM change_planner_plan_version",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
fn lineage_heads_do_not_read_the_body_column() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = database(&dir.path().join("heads.db"))?;
    add_ruleset(&db)?;
    db.connection
        .execute_batch("PRAGMA ignore_check_constraints=ON;")?;
    db.connection.execute(
        "INSERT INTO change_planner_plan_version
         (plan_version_id,lineage_id,version_hash,hash_format,ruleset_version,
          readiness,origin_channel,body,created_at_ms)
         VALUES (?1,?2,?3,'du.plan.hash.v1',1,'ready','gui','{',100)",
        params![blob(&id(31))?, blob(&id(30))?, [31u8; 32].as_slice()],
    )?;

    let heads = db
        .read_lineage_heads(&id(30))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0].plan_version_id, id(31));
    Ok(())
}

#[test]
fn duplicate_plan_version_id_returns_a_distinct_error() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = database(&dir.path().join("duplicate.db"))?;
    add_ruleset(&db)?;
    let version = record(8, 80, None, 100);
    db.append_fixed(&version)
        .map_err(|error| format!("{error:?}"))?;

    let error = db
        .append_fixed(&version)
        .err()
        .ok_or("duplicate plan version ID was stored")?;
    assert_eq!(error.kind, PortErrorKind::InUse);
    assert_eq!(error.reason_key, "db.duplicate");
    Ok(())
}

#[test]
fn immutable_rows_and_ruleset_restrict_are_enforced() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = database(&dir.path().join("immutable.db"))?;
    add_ruleset(&db)?;
    let version = record(4, 40, None, 100);
    db.append_fixed(&version)
        .map_err(|error| format!("{error:?}"))?;

    assert!(
        db.connection
            .execute("UPDATE change_planner_plan_version SET body = 'null'", [])
            .is_err()
    );
    assert!(
        db.connection
            .execute("DELETE FROM change_planner_plan_version", [])
            .is_err()
    );
    let replace = "INSERT OR REPLACE INTO change_planner_plan_version
        (plan_version_id,lineage_id,version_hash,hash_format,ruleset_version,
         readiness,origin_channel,body,created_at_ms)
        VALUES (?1,?2,?3,'du.plan.hash.v1',1,'ready','gui','{}',100)";
    assert!(
        db.connection
            .execute(
                replace,
                params![
                    blob(&version.plan_version_id)?,
                    blob(&version.lineage_id)?,
                    [4u8; 32].as_slice()
                ],
            )
            .is_err()
    );

    db.connection
        .execute_batch("DROP TRIGGER change_planner_ruleset_no_delete;")?;
    assert!(
        db.connection
            .execute("DELETE FROM change_planner_ruleset WHERE version = 1", [])
            .is_err(),
        "the plan's ruleset reference must be RESTRICT"
    );
    Ok(())
}

#[test]
fn plan_schema_rejects_bad_enums_and_blob_lengths() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = database(&dir.path().join("constraints.db"))?;
    add_ruleset(&db)?;

    for (plan_id, hash, readiness, channel, body) in [
        (vec![1u8; 16], vec![1u8; 32], "unknown", "gui", "{}"),
        (vec![2u8; 16], vec![1u8; 32], "ready", "unknown", "{}"),
        (vec![3u8; 15], vec![1u8; 32], "ready", "gui", "{}"),
        (vec![4u8; 16], vec![1u8; 31], "ready", "gui", "{}"),
        (vec![5u8; 16], vec![1u8; 32], "ready", "gui", "{"),
    ] {
        assert!(
            db.connection
                .execute(
                    "INSERT INTO change_planner_plan_version
                     (plan_version_id,lineage_id,version_hash,hash_format,ruleset_version,
                      readiness,origin_channel,body,created_at_ms)
                     VALUES (?1,zeroblob(16),?2,'du.plan.hash.v1',1,?3,?4,?5,1)",
                    params![plan_id, hash, readiness, channel, body],
                )
                .is_err()
        );
    }
    assert!(
        db.connection
            .execute(
                "INSERT INTO change_planner_plan_version
         (plan_version_id,lineage_id,version_hash,hash_format,ruleset_version,
          readiness,origin_channel,body,created_at_ms)
         VALUES (zeroblob(16),zeroblob(16),zeroblob(32),'',1,'ready','gui','{}',1)",
                [],
            )
            .is_err()
    );
    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM change_planner_plan_version",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
fn missing_ruleset_and_malformed_body_fail_without_partial_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = database(&dir.path().join("rollback.db"))?;
    let missing_ruleset = record(6, 60, None, 100);
    assert!(db.append_fixed(&missing_ruleset).is_err());

    let mut malformed = record(7, 60, None, 100);
    malformed.body = "{".into();
    assert!(db.append_fixed(&malformed).is_err());

    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM change_planner_plan_version",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
#[ignore = "opt-in 100,000-item plan storage and read measurement"]
fn plan_version_measures_a_near_limit_body() -> TestResult {
    use std::{process::Command, time::Instant};

    const ITEM_COUNT: usize = 100_000;
    const PATH_BYTES: usize = 1_024;
    let target_bytes = MAX_PLAN_BODY_BYTES - 64 * 1024;
    let body_prefix = "{\"items\":[";
    let item_prefix = "{\"path\":\"";
    let item_middle = "\",\"detail\":\"";
    let item_suffix = "\"}";
    let body_suffix = "]}";
    let base_bytes = body_prefix.len()
        + body_suffix.len()
        + ITEM_COUNT * (item_prefix.len() + PATH_BYTES + item_middle.len() + item_suffix.len())
        + (ITEM_COUNT - 1);
    let fill_bytes = target_bytes - base_bytes;
    let fill_per_item = fill_bytes / ITEM_COUNT;
    let final_item_fill = fill_bytes % ITEM_COUNT;
    let path = "p".repeat(PATH_BYTES);
    let fill = "x".repeat(fill_per_item);
    let final_fill = "x".repeat(final_item_fill);

    let started = Instant::now();
    let mut body = String::with_capacity(target_bytes);
    body.push_str(body_prefix);
    for index in 0..ITEM_COUNT {
        if index > 0 {
            body.push(',');
        }
        body.push_str(item_prefix);
        body.push_str(&path);
        body.push_str(item_middle);
        body.push_str(&fill);
        if index + 1 == ITEM_COUNT {
            body.push_str(&final_fill);
        }
        body.push_str(item_suffix);
    }
    body.push_str(body_suffix);
    assert_eq!(body.len(), target_bytes);
    assert!(body.len() < MAX_PLAN_BODY_BYTES);
    let build_body_ms = started.elapsed().as_millis();

    let dir = tempfile::tempdir()?;
    let mut db = database(&dir.path().join("maximum-plan.db"))?;
    add_ruleset(&db)?;
    let mut version = record(32, 32, None, 100);
    version.body = body;

    let append_started = Instant::now();
    db.append_fixed(&version)
        .map_err(|error| format!("{error:?}"))?;
    let append_ms = append_started.elapsed().as_millis();

    let read_started = Instant::now();
    let loaded = db
        .read(&version.plan_version_id)
        .map_err(|error| format!("{error:?}"))?;
    let read_ms = read_started.elapsed().as_millis();
    assert_eq!(loaded, version);

    let peak_working_set_bytes = {
        let command = format!("(Get-Process -Id {}).PeakWorkingSet64", std::process::id());
        Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &command])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                String::from_utf8(output.stdout)
                    .ok()?
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
    };
    println!(
        "plan storage measurement: items={ITEM_COUNT}, body_bytes={}, build_body_ms={build_body_ms}, append_ms={append_ms}, read_ms={read_ms}, total_ms={}, peak_working_set_bytes={peak_working_set_bytes:?}",
        version.body.len(),
        started.elapsed().as_millis()
    );
    Ok(())
}
