use super::{AppDb, TestResult, id, insert_history, writer};
use du_core_contracts::Id;
use rusqlite::params;

fn insert_approval_check(
    db: &AppDb,
    seed: u8,
    job_id: Option<&Id>,
    created_at_ms: i64,
) -> TestResult {
    let check_id = id(seed).v7_bytes().ok_or("invalid check id")?;
    let job_id = job_id
        .map(|job| {
            job.v7_bytes()
                .map(|bytes| bytes.to_vec())
                .ok_or("invalid job id")
        })
        .transpose()?;
    db.connection.execute(
        "INSERT INTO job_runtime_approval_check
         (check_id, job_id, request_version_id, request_kind, version_hash,
          hash_format, operation_kind, record_kind, check_result, mismatch_fields,
          approved_at_ms, created_at_ms, correlation_id)
         VALUES (?1, ?2, zeroblob(16), 'plan', zeroblob(32), 'du.plan.hash.v1',
                 'delete', 'start', 'valid', '[]', 1, ?3, zeroblob(16))",
        params![check_id.as_slice(), job_id, created_at_ms],
    )?;
    Ok(())
}

#[test]
fn expired_unattached_approval_checks_use_created_at_and_attached_rows_follow_the_job() -> TestResult
{
    let dir = tempfile::tempdir()?;
    let mut db = writer(&dir.path().join("approval-retention.db"))?;
    let reference_ms = 8_000_000_000_i64;
    let cutoff_ms = reference_ms - 90 * 86_400_000;
    let linked_job = id(31);

    insert_approval_check(&db, 32, None, cutoff_ms)?;
    insert_approval_check(&db, 33, None, cutoff_ms + 1)?;
    insert_approval_check(&db, 34, Some(&linked_job), cutoff_ms + 1)?;
    insert_history(&mut db, &linked_job, cutoff_ms - 1, 1, 0)?;

    let unattached_receipt = db
        .delete_expired_history_batch(&[], 90, reference_ms, reference_ms + 1)
        .map_err(|error| format!("{error:?}"))?;
    assert!(unattached_receipt.deleted_job_ids.is_empty());
    assert_eq!(unattached_receipt.deleted_row_count, 1);

    let selected = db.read_history_prune_candidates(90, reference_ms, None, 10)?;

    let receipt = db
        .delete_expired_history_batch(&selected, 90, reference_ms, reference_ms + 1)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        receipt.deleted_job_ids.as_slice(),
        std::slice::from_ref(&linked_job)
    );
    assert_eq!(receipt.deleted_row_count, 4);

    let remaining: Vec<(String, i64)> = db
        .connection
        .prepare(
            "SELECT coalesce(hex(job_id), ''), created_at_ms
             FROM job_runtime_approval_check ORDER BY created_at_ms",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    assert_eq!(remaining, [(String::new(), cutoff_ms + 1)]);
    Ok(())
}

#[test]
fn unattached_approval_retention_is_fixed_at_90_days() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = writer(&dir.path().join("fixed-approval-retention.db"))?;
    let reference_ms = 8_000_000_000_i64;
    let cutoff_ms = reference_ms - 90 * 86_400_000;
    insert_approval_check(&db, 35, None, cutoff_ms)?;
    insert_approval_check(&db, 36, None, cutoff_ms + 1)?;

    let receipt = db
        .delete_expired_history_batch(&[], 0, reference_ms, reference_ms + 1)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(receipt.deleted_row_count, 1);
    let remaining: Vec<i64> = db
        .connection
        .prepare("SELECT created_at_ms FROM job_runtime_approval_check")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    assert_eq!(remaining, [cutoff_ms + 1]);
    Ok(())
}

#[test]
fn orphaned_job_approval_rows_expire_after_90_days() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = writer(&dir.path().join("orphan-approval-retention.db"))?;
    let reference_ms = 8_000_000_000_i64;
    let cutoff_ms = reference_ms - 90 * 86_400_000;
    let orphan_job = id(41);
    let live_job = id(42);

    insert_approval_check(&db, 43, Some(&orphan_job), cutoff_ms)?;
    insert_approval_check(&db, 44, Some(&orphan_job), cutoff_ms + 1)?;
    insert_approval_check(&db, 45, Some(&live_job), cutoff_ms)?;
    insert_history(&mut db, &live_job, cutoff_ms - 1, 1, 0)?;

    let receipt = db
        .delete_expired_history_batch(&[], 0, reference_ms, reference_ms + 1)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(receipt.deleted_row_count, 1);
    let remaining: Vec<(String, i64)> = db
        .connection
        .prepare(
            "SELECT hex(check_id), created_at_ms FROM job_runtime_approval_check
             ORDER BY created_at_ms, check_id",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let check_id = |seed: u8| {
        let mut bytes = [seed; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>()
    };
    assert_eq!(
        remaining,
        [(check_id(45), cutoff_ms), (check_id(44), cutoff_ms + 1),]
    );
    Ok(())
}
