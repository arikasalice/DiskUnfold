use super::*;
use du_core_contracts::{
    history_ipc::{HistoryItemOutcome, HistoryRelation},
    job::{
        DisplacedItem, ItemHistoryRecord, ItemHistoryStart, JobCounts, JobState, JobSummary,
        ObservedSize,
    },
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn id(value: u8) -> Id {
    let mut bytes = [0; 16];
    bytes[15] = value;
    Id::from_v7_bytes(bytes)
}

fn per_item_start(value: u8) -> ItemHistoryStart {
    ItemHistoryStart {
        job_id: id(value),
        kind_id: "change.trash".into(),
        kind_version: 1,
        scope_summary: "3 items".into(),
        started_at_ms: 100,
        plan_version_id: Some(id(90)),
        plan_lineage_id: Some(id(91)),
        plan_hash: Some(vec![7; 32]),
        predecessor_job_id: Some(id(92)),
        relation_kind: Some(HistoryRelation::Retry),
    }
}

fn item(sequence: u64, outcome: HistoryItemOutcome) -> ItemHistoryRecord {
    ItemHistoryRecord {
        item_seq: sequence,
        item_ref: vec![sequence as u8 + 1],
        location_raw: vec![b'/', sequence as u8 + b'1'],
        location_text: format!("item-{}", sequence + 1),
        outcome,
        reason_key: None,
        review_needed: false,
        held: false,
        outcome_unknown: false,
        observed_size: ObservedSize::Exact(sequence + 10),
        displaced: None,
    }
}

fn summary(start: &ItemHistoryStart, completed: u64, unprocessed: u64) -> JobSummary {
    JobSummary {
        job_id: start.job_id.clone(),
        kind_id: start.kind_id.clone(),
        kind_version: start.kind_version,
        scope_summary: start.scope_summary.clone(),
        scope_roots: vec!["/fixture/per-item".into()],
        state: if unprocessed == 0 {
            JobState::Done
        } else {
            JobState::Failed
        },
        counts: JobCounts {
            completed,
            unprocessed,
            ..JobCounts::default()
        },
        failure_reason_key: (unprocessed > 0).then(|| "history.record_failed".into()),
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(start.started_at_ms),
        finished_at_ms: 200,
        correlation_id: start.job_id.clone(),
    }
}

fn database() -> Result<(tempfile::TempDir, AppDb, std::path::PathBuf), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("item-history.db");
    let db = AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?;
    Ok((dir, db, path))
}

fn read_page(
    path: &std::path::Path,
    job_id: &Id,
) -> Result<super::super::read::items::Page, Box<dyn std::error::Error>> {
    let mut reader = AppDb::open_read_only_live(path).map_err(|error| format!("{error:?}"))?;
    reader
        .read_history_items(job_id, None, None, 10)
        .map_err(|error| format!("{error:?}").into())
}

fn read_list_page(
    path: &std::path::Path,
) -> Result<super::super::read::Page, Box<dyn std::error::Error>> {
    let mut reader = AppDb::open_read_only_live(path).map_err(|error| format!("{error:?}"))?;
    reader
        .read_history_page(super::super::read::Filter::default(), None, 10)
        .map_err(|error| format!("{error:?}").into())
}

#[test]
fn appends_start_items_and_terminal_and_reads_items_in_order() -> TestResult {
    let (_dir, mut db, path) = database()?;
    let start = per_item_start(1);
    db.append_item_history_start(&start, 101)?;
    db.append_item_history_result(&start.job_id, &item(0, HistoryItemOutcome::Completed), 102)?;
    db.append_item_history_result(&start.job_id, &item(1, HistoryItemOutcome::Skipped), 103)?;
    db.append_item_history_result(&start.job_id, &item(2, HistoryItemOutcome::Failed), 104)?;
    assert!(db.append_scan_history(&summary(&start, 3, 0), 201).is_ok());
    let linkage: (String, Vec<u8>, Vec<u8>, Vec<u8>, String) = db.connection.query_row(
        "SELECT record_origin, plan_version_id, plan_lineage_id, plan_hash, relation_kind
         FROM job_runtime_job_link WHERE job_id=?1",
        [id_bytes(&start.job_id).ok_or("invalid test ID")?.as_slice()],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    assert_eq!(
        linkage,
        (
            "this_app".into(),
            id_bytes(start.plan_version_id.as_ref().ok_or("missing plan ID")?)
                .ok_or("invalid plan ID")?
                .to_vec(),
            id_bytes(start.plan_lineage_id.as_ref().ok_or("missing lineage ID")?)
                .ok_or("invalid lineage ID")?
                .to_vec(),
            vec![7; 32],
            "retry".into()
        )
    );
    let root_count: i64 = db.connection.query_row(
        "SELECT count(*) FROM job_runtime_job_scope_root WHERE job_id=?1",
        [id_bytes(&start.job_id).ok_or("invalid test ID")?.as_slice()],
        |row| row.get(0),
    )?;
    assert_eq!(root_count, 1);
    let page = read_page(&path, &start.job_id)?;
    assert!(page.item_history_expected);
    assert_eq!((page.recorded, page.summary), (3, Some(3)));
    assert_eq!(
        page.rows.iter().map(|row| row.item_seq).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    Ok(())
}

#[test]
fn rejects_out_of_order_duplicates_and_check_incompatible_displacement() -> TestResult {
    let (_dir, mut db, _) = database()?;
    let start = per_item_start(2);
    db.append_item_history_start(&start, 101)?;
    assert_eq!(
        db.append_item_history_result(&start.job_id, &item(1, HistoryItemOutcome::Completed), 102),
        Err(ItemHistoryError::OutOfOrder)
    );
    db.append_item_history_result(&start.job_id, &item(0, HistoryItemOutcome::Completed), 102)?;
    assert_eq!(
        db.append_item_history_result(&start.job_id, &item(0, HistoryItemOutcome::Completed), 103),
        Err(ItemHistoryError::Duplicate)
    );
    let count: i64 = db.connection.query_row(
        "SELECT count(*) FROM job_runtime_item_result WHERE job_id=?1",
        [id_bytes(&start.job_id).ok_or("invalid test ID")?.as_slice()],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);

    let mut invalid = item(1, HistoryItemOutcome::Overwritten);
    assert_eq!(
        db.append_item_history_result(&start.job_id, &invalid, 104),
        Err(ItemHistoryError::Invalid)
    );
    invalid.outcome = HistoryItemOutcome::Completed;
    invalid.displaced = Some(DisplacedItem {
        trash_ref: None,
        name: None,
        original_location: None,
        trashed_at_ms: None,
    });
    assert_eq!(
        db.append_item_history_result(&start.job_id, &invalid, 105),
        Err(ItemHistoryError::Invalid)
    );
    let mut valid = item(1, HistoryItemOutcome::Overwritten);
    valid.displaced = Some(DisplacedItem {
        trash_ref: Some(vec![8]),
        name: Some("old-name".into()),
        original_location: Some("old-location".into()),
        trashed_at_ms: Some(150),
    });
    db.append_item_history_result(&start.job_id, &valid, 106)?;
    Ok(())
}

#[test]
fn failed_item_transaction_leaves_gap_and_zero_item_start_is_recorded() -> TestResult {
    let (_dir, mut db, path) = database()?;
    let start = per_item_start(3);
    db.append_item_history_start(&start, 101)?;
    db.append_item_history_result(&start.job_id, &item(0, HistoryItemOutcome::Completed), 102)?;
    db.connection.execute_batch(
        "CREATE TRIGGER fail_item_one BEFORE INSERT ON job_runtime_item_result
         WHEN NEW.item_seq=1 BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    )?;
    assert_eq!(
        db.append_item_history_result(&start.job_id, &item(1, HistoryItemOutcome::Completed), 103),
        Err(ItemHistoryError::Unavailable)
    );
    db.connection.execute_batch("DROP TRIGGER fail_item_one;")?;
    db.append_scan_history(&summary(&start, 1, 2), 201)?;
    let page = read_page(&path, &start.job_id)?;
    assert!(page.item_history_expected);
    assert_eq!((page.recorded, page.summary), (1, Some(3)));

    let (_zero_dir, mut zero_db, zero_path) = database()?;
    let zero_start = per_item_start(4);
    zero_db.append_item_history_start(&zero_start, 101)?;
    zero_db.append_scan_history(&summary(&zero_start, 0, 0), 201)?;
    let zero_page = read_page(&zero_path, &zero_start.job_id)?;
    assert!(zero_page.item_history_expected);
    assert_eq!((zero_page.recorded, zero_page.summary), (0, Some(0)));
    Ok(())
}

#[test]
fn item_rows_do_not_advance_the_history_list_revision() -> TestResult {
    let (_dir, mut db, path) = database()?;
    let start = per_item_start(7);
    let initial = read_list_page(&path)?;
    assert_eq!(initial.revision.max_record_seq, 0);
    assert!(initial.rows.is_empty());

    db.append_item_history_start(&start, 101)?;
    assert_eq!(read_list_page(&path)?.revision.max_record_seq, 0);
    db.append_item_history_result(&start.job_id, &item(0, HistoryItemOutcome::Completed), 102)?;
    assert_eq!(read_list_page(&path)?.revision.max_record_seq, 0);
    db.append_item_history_result(&start.job_id, &item(1, HistoryItemOutcome::Completed), 103)?;
    assert_eq!(read_list_page(&path)?.revision.max_record_seq, 0);

    db.append_scan_history(&summary(&start, 2, 0), 201)?;
    let ended = read_list_page(&path)?;
    assert_eq!(ended.revision.max_record_seq, 1);
    assert_eq!(ended.rows.len(), 1);
    assert_eq!(ended.rows[0].position.record_seq, 1);
    Ok(())
}

#[test]
fn item_check_constraints_allow_failed_displacement_but_reject_invalid_sizes() -> TestResult {
    let (_dir, mut db, _) = database()?;
    let start = per_item_start(6);
    db.append_item_history_start(&start, 101)?;
    let mut failed = item(0, HistoryItemOutcome::Failed);
    failed.displaced = Some(DisplacedItem {
        trash_ref: None,
        name: Some("displaced".into()),
        original_location: None,
        trashed_at_ms: None,
    });
    db.append_item_history_result(&start.job_id, &failed, 102)?;
    let too_large = ItemHistoryRecord {
        item_seq: 1,
        observed_size: ObservedSize::Exact(u64::MAX),
        ..item(1, HistoryItemOutcome::Completed)
    };
    assert_eq!(
        db.append_item_history_result(&start.job_id, &too_large, 103),
        Err(ItemHistoryError::Invalid)
    );
    Ok(())
}

#[test]
fn rejects_empty_and_oversized_item_fields_as_invalid() -> TestResult {
    let (_dir, mut db, _) = database()?;
    let start = per_item_start(8);
    db.append_item_history_start(&start, 101)?;

    let mut invalid = item(0, HistoryItemOutcome::Completed);
    invalid.item_ref.clear();
    assert_eq!(
        db.append_item_history_result(&start.job_id, &invalid, 102),
        Err(ItemHistoryError::Invalid)
    );
    invalid = item(0, HistoryItemOutcome::Completed);
    invalid.item_ref = vec![0; 1_025];
    assert_eq!(
        db.append_item_history_result(&start.job_id, &invalid, 103),
        Err(ItemHistoryError::Invalid)
    );
    invalid = item(0, HistoryItemOutcome::Completed);
    invalid.location_text = "x".repeat(32_769);
    assert_eq!(
        db.append_item_history_result(&start.job_id, &invalid, 104),
        Err(ItemHistoryError::Invalid)
    );
    assert_eq!(
        db.append_item_history_result(&start.job_id, &item(0, HistoryItemOutcome::Completed), 105),
        Ok(())
    );
    Ok(())
}

#[test]
fn interrupted_start_keeps_prior_item_rows() -> TestResult {
    let (_dir, mut db, path) = database()?;
    let start = per_item_start(5);
    db.append_item_history_start(&start, 101)?;
    db.append_item_history_result(&start.job_id, &item(0, HistoryItemOutcome::Completed), 102)?;
    let receipt = db
        .mark_interrupted_jobs(500)?
        .ok_or("missing interruption")?;
    assert_eq!(receipt.count, 1);
    let page = read_page(&path, &start.job_id)?;
    assert!(page.item_history_expected);
    assert_eq!(page.recorded, 1);
    assert_eq!(page.rows[0].item_seq, 0);
    Ok(())
}
