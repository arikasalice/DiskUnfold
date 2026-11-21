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
