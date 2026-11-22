use super::MAX_APPROVAL_SUMMARY_BYTES;
use crate::AppDb;
use du_core_contracts::{
    Id,
    approval_check::{
        ApprovalCheckFacts, ApprovalCheckFinishRecord, ApprovalCheckRecordKind,
        ApprovalCheckRecorder, ApprovalCheckResult, ApprovalCheckStartRecord,
        ApprovalOperationKind, ApprovalRequestKind,
    },
    ports::PortErrorKind,
};
use rusqlite::params;
use std::sync::{Arc, Barrier};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Clone)]
struct RawCheck {
    check_id: Vec<u8>,
    job_id: Option<Vec<u8>>,
    request_version_id: Vec<u8>,
    request_kind: &'static str,
    version_hash: Vec<u8>,
    hash_format: &'static str,
    operation_kind: &'static str,
    destination_summary: Option<&'static str>,
    keep_summary: Option<&'static str>,
    record_kind: &'static str,
    check_result: &'static str,
    mismatch_fields: &'static str,
    end_state: Option<&'static str>,
    approved_at_ms: i64,
    created_at_ms: i64,
    correlation_id: Vec<u8>,
}

fn id(seed: u8) -> Id {
    let mut bytes = [0; 16];
    bytes[0] = seed;
    Id::from_v7_bytes(bytes)
}

fn blob(value: &Id) -> Vec<u8> {
    value
        .v7_bytes()
        .map_or_else(Vec::new, |bytes| bytes.to_vec())
}

fn facts(seed: u8, job_id: Option<Id>, created_at_ms: i64) -> ApprovalCheckFacts {
    ApprovalCheckFacts {
        check_id: id(seed),
        job_id,
        request_version_id: id(20),
        request_kind: ApprovalRequestKind::Plan,
        version_hash: [seed; 32],
        hash_format: "du.plan.hash.v1".into(),
        operation_kind: ApprovalOperationKind::Delete,
        destination_summary: Some(r#"{"kind":"none"}"#.into()),
        keep_summary: None,
        check_result: ApprovalCheckResult::Valid,
        mismatch_fields: "[]".into(),
        approved_at_ms: 100,
        created_at_ms,
        correlation_id: id(30),
    }
}

fn raw(seed: u8) -> RawCheck {
    RawCheck {
        check_id: vec![seed; 16],
        job_id: Some(vec![40; 16]),
        request_version_id: vec![20; 16],
        request_kind: "plan",
        version_hash: vec![seed; 32],
        hash_format: "du.plan.hash.v1",
        operation_kind: "delete",
        destination_summary: Some(r#"{"kind":"none"}"#),
        keep_summary: None,
        record_kind: "start",
        check_result: "valid",
        mismatch_fields: "[]",
        end_state: None,
        approved_at_ms: 100,
        created_at_ms: i64::from(seed),
        correlation_id: vec![30; 16],
    }
}

fn insert_raw(db: &AppDb, row: &RawCheck) -> rusqlite::Result<usize> {
    db.connection.execute(
        "INSERT INTO job_runtime_approval_check
         (check_id,job_id,request_version_id,request_kind,version_hash,hash_format,
          operation_kind,destination_summary,keep_summary,record_kind,check_result,
          mismatch_fields,end_state,approved_at_ms,created_at_ms,correlation_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            row.check_id,
            row.job_id,
            row.request_version_id,
            row.request_kind,
            row.version_hash,
            row.hash_format,
            row.operation_kind,
            row.destination_summary,
            row.keep_summary,
            row.record_kind,
            row.check_result,
            row.mismatch_fields,
            row.end_state,
            row.approved_at_ms,
            row.created_at_ms,
            row.correlation_id,
        ],
    )
}

#[test]
fn start_finish_round_trip_and_both_lookup_indexes() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("approval.db");
    let mut db = AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let job = id(40);
    let start_facts = facts(1, Some(job.clone()), 100);
    let tied_facts = facts(3, Some(job.clone()), 100);
    let finish_facts = facts(2, Some(job.clone()), 200);
    db.append_start(&ApprovalCheckStartRecord {
        facts: start_facts.clone(),
    })
    .map_err(|error| format!("{error:?}"))?;
    db.append_start(&ApprovalCheckStartRecord {
        facts: tied_facts.clone(),
    })
    .map_err(|error| format!("{error:?}"))?;
    db.append_finish(&ApprovalCheckFinishRecord {
        facts: finish_facts.clone(),
        end_state: "done".into(),
    })
    .map_err(|error| format!("{error:?}"))?;

    let request_rows = db
        .read_for_request(&id(20))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(request_rows.len(), 3);
    assert_eq!(request_rows[0].facts, start_facts);
    assert_eq!(request_rows[0].record_kind, ApprovalCheckRecordKind::Start);
    assert_eq!(request_rows[1].facts, tied_facts);
    assert_eq!(request_rows[1].record_kind, ApprovalCheckRecordKind::Start);
    assert_eq!(request_rows[2].facts, finish_facts);
    assert_eq!(request_rows[2].record_kind, ApprovalCheckRecordKind::Finish);
    assert_eq!(request_rows[2].end_state.as_deref(), Some("done"));
    assert_eq!(
        db.read_for_job(&job)
            .map_err(|error| format!("{error:?}"))?
            .len(),
        3
    );

    let request_plan: String = db.connection.query_row(
        "EXPLAIN QUERY PLAN
         SELECT check_id FROM job_runtime_approval_check
         WHERE request_version_id = ?1 ORDER BY created_at_ms, check_id",
        [blob(&id(20))],
        |row| row.get(3),
    )?;
    assert!(
        request_plan.contains("ix_job_runtime_approval_check_request"),
        "{request_plan}"
    );
    assert!(!request_plan.contains("USE TEMP B-TREE"), "{request_plan}");
    let job_plan: String = db.connection.query_row(
        "EXPLAIN QUERY PLAN
         SELECT check_id FROM job_runtime_approval_check
         WHERE job_id = ?1 ORDER BY created_at_ms, check_id",
        [blob(&job)],
        |row| row.get(3),
    )?;
    assert!(
        job_plan.contains("ix_job_runtime_approval_check_job"),
        "{job_plan}"
    );
    assert!(!job_plan.contains("USE TEMP B-TREE"), "{job_plan}");
    Ok(())
}

#[test]
fn append_only_updates_closed_deletes_and_replace_are_rejected() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("append-only.db"))
        .map_err(|error| format!("{error:?}"))?;
    let record = facts(3, Some(id(40)), 100);
    db.append_start(&ApprovalCheckStartRecord {
        facts: record.clone(),
    })
    .map_err(|error| format!("{error:?}"))?;
    let key = blob(&record.check_id);
    assert!(
        db.connection
            .execute(
                "UPDATE job_runtime_approval_check SET check_result = 'other' WHERE check_id = ?1",
                [&key],
            )
            .is_err()
    );
    assert!(
        db.connection
            .execute("DELETE FROM job_runtime_approval_check", [])
            .is_err()
    );
    assert!(
        db.connection
            .execute(
                "INSERT OR REPLACE INTO job_runtime_approval_check
                 (check_id,job_id,request_version_id,request_kind,version_hash,hash_format,
                  operation_kind,record_kind,check_result,mismatch_fields,approved_at_ms,
                  created_at_ms,correlation_id)
                 VALUES (?1,zeroblob(16),zeroblob(16),'plan',zeroblob(32),
                         'du.plan.hash.v1','delete','start','valid','[]',1,1,zeroblob(16))",
                [&key],
            )
            .is_err()
    );

    db.connection.execute(
        "UPDATE app_db_purge_gate SET open = 1 WHERE gate_key = 1",
        [],
    )?;
    db.connection
        .execute("DELETE FROM job_runtime_approval_check", [])?;
    db.connection.execute(
        "UPDATE app_db_purge_gate SET open = 0 WHERE gate_key = 1",
        [],
    )?;
    Ok(())
}

#[test]
fn concurrent_appends_are_serialized_and_preserved() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("concurrent.db");
    drop(AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?);
    let left_db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let right_db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let barrier = Arc::new(Barrier::new(3));
    let left_barrier = Arc::clone(&barrier);
    let right_barrier = Arc::clone(&barrier);
    let left_facts = facts(11, Some(id(40)), 300);
    let right_facts = facts(12, Some(id(40)), 300);

    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(move || {
            let mut db = left_db;
            left_barrier.wait();
            db.append_start(&ApprovalCheckStartRecord { facts: left_facts })
        });
        let right = scope.spawn(move || {
            let mut db = right_db;
            right_barrier.wait();
            db.append_start(&ApprovalCheckStartRecord { facts: right_facts })
        });
        barrier.wait();
        (left.join(), right.join())
    });
    assert!(left.is_ok_and(|result| result.is_ok()));
    assert!(right.is_ok_and(|result| result.is_ok()));

    let db = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        db.read_for_request(&id(20))
            .map_err(|error| format!("{error:?}"))?
            .len(),
        2
    );
    Ok(())
}

#[test]
fn lock_timeout_returns_busy_without_appending() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("busy.db");
    let blocker = AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    let mut writer = AppDb::open_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    blocker.connection.execute_batch("BEGIN IMMEDIATE")?;

    let error = writer
        .append_start(&ApprovalCheckStartRecord {
            facts: facts(13, Some(id(40)), 400),
        })
        .err()
        .ok_or("append succeeded while another writer held the lock")?;
    assert_eq!(error.kind, PortErrorKind::Busy);
    assert_eq!(error.reason_key, "db.busy");
    blocker.connection.execute_batch("ROLLBACK")?;

    assert!(
        writer
            .read_for_request(&id(20))
            .map_err(|error| format!("{error:?}"))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn caller_input_violations_are_rejected_before_writing() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("invalid-input.db"))
        .map_err(|error| format!("{error:?}"))?;

    let mut known_field = facts(20, Some(id(40)), 100);
    known_field.mismatch_fields = r#"["plan_hash"]"#.into();
    db.append_start(&ApprovalCheckStartRecord { facts: known_field })
        .map_err(|error| format!("{error:?}"))?;

    let mut invalid_summary = facts(21, Some(id(40)), 100);
    invalid_summary.destination_summary = Some("not-json".into());
    let mut invalid_field_key = facts(22, Some(id(40)), 100);
    invalid_field_key.mismatch_fields = r#"["C:\\Users\\private\\secret.txt"]"#.into();
    let mut non_string_field = facts(23, Some(id(40)), 100);
    non_string_field.mismatch_fields = "[1]".into();
    let mut oversized_destination = facts(24, Some(id(40)), 100);
    oversized_destination.destination_summary =
        Some(format!("[{}]", " ".repeat(MAX_APPROVAL_SUMMARY_BYTES + 1)));
    let mut oversized_keep = facts(25, Some(id(40)), 100);
    oversized_keep.keep_summary = Some(format!("[{}]", " ".repeat(MAX_APPROVAL_SUMMARY_BYTES + 1)));

    for invalid in [
        invalid_summary,
        invalid_field_key,
        non_string_field,
        oversized_destination,
        oversized_keep,
    ] {
        let error = db
            .append_start(&ApprovalCheckStartRecord { facts: invalid })
            .err()
            .ok_or("invalid approval check was stored")?;
        assert_eq!(error.kind, PortErrorKind::Unsupported);
        assert_eq!(error.reason_key, "db.invalid_input");
    }

    let empty_end_state = ApprovalCheckFinishRecord {
        facts: facts(26, Some(id(40)), 100),
        end_state: String::new(),
    };
    let error = db
        .append_finish(&empty_end_state)
        .err()
        .ok_or("empty end_state was stored")?;
    assert_eq!(error.kind, PortErrorKind::Unsupported);
    assert_eq!(error.reason_key, "db.invalid_input");
    Ok(())
}

#[test]
fn duplicate_check_id_returns_a_distinct_error() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = AppDb::create_gui_with_history(&dir.path().join("duplicate.db"))
        .map_err(|error| format!("{error:?}"))?;
    let record = ApprovalCheckStartRecord {
        facts: facts(27, Some(id(40)), 100),
    };
    db.append_start(&record)
        .map_err(|error| format!("{error:?}"))?;

    let error = db
        .append_start(&record)
        .err()
        .ok_or("duplicate check ID was stored")?;
    assert_eq!(error.kind, PortErrorKind::InUse);
    assert_eq!(error.reason_key, "db.duplicate");
    Ok(())
}

#[test]
fn write_check_constraint_is_not_reported_as_corruption() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = AppDb::create_gui_with_history(&dir.path().join("check-constraint.db"))
        .map_err(|error| format!("{error:?}"))?;
    let mut invalid = raw(28);
    invalid.check_result = "unknown_result";
    let sqlite_error = insert_raw(&db, &invalid)
        .err()
        .ok_or("CHECK violation should fail")?;
    let error = super::db_write_error(sqlite_error);
    assert_eq!(error.kind, PortErrorKind::Unsupported);
    assert_eq!(error.reason_key, "db.invalid_input");
    Ok(())
}

#[test]
fn approval_schema_rejects_enums_blob_lengths_and_invalid_json() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = AppDb::create_gui_with_history(&dir.path().join("constraints.db"))
        .map_err(|error| format!("{error:?}"))?;

    let mut bad = raw(1);
    bad.request_kind = "unknown";
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(2);
    bad.operation_kind = "unknown";
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(3);
    bad.record_kind = "unknown";
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(4);
    bad.check_id = vec![4; 15];
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(5);
    bad.request_version_id = vec![5; 15];
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(6);
    bad.version_hash = vec![6; 31];
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(7);
    bad.destination_summary = Some("not-json");
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(8);
    bad.mismatch_fields = "{}";
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(9);
    bad.check_result = "";
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(10);
    bad.record_kind = "finish";
    assert!(insert_raw(&db, &bad).is_err());
    let mut bad = raw(11);
    bad.record_kind = "finish";
    bad.end_state = Some("");
    assert!(insert_raw(&db, &bad).is_err());

    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM job_runtime_approval_check",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    let foreign_keys: i64 = db.connection.query_row(
        "SELECT count(*) FROM pragma_foreign_key_list('job_runtime_approval_check')",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(foreign_keys, 0);
    Ok(())
}

#[test]
fn approval_schema_accepts_known_check_results_and_rejects_unknown_values() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = AppDb::create_gui_with_history(&dir.path().join("check-results.db"))
        .map_err(|error| format!("{error:?}"))?;

    for (index, value) in [
        "valid",
        "not_issued",
        "already_used",
        "mismatch",
        "confirmation_missing",
    ]
    .into_iter()
    .enumerate()
    {
        let seed = u8::try_from(index + 1)?;
        let mut row = raw(seed);
        row.check_result = value;
        assert_eq!(insert_raw(&db, &row)?, 1, "{value}");
    }

    let mut unknown = raw(6);
    unknown.check_result = "unknown_value";
    assert!(insert_raw(&db, &unknown).is_err());

    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM job_runtime_approval_check",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 5);
    Ok(())
}
