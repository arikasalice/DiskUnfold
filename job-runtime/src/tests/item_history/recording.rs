use super::*;

struct ScriptedBody {
    records: Vec<ItemHistoryRecord>,
    results: RecordResults,
    checkpoints: Option<Arc<Mutex<Vec<Checkpoint>>>>,
}

impl JobBody for ScriptedBody {
    fn kind_id(&self) -> &str {
        "change.trash"
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        for record in &self.records {
            let result = context.record_item(record.clone());
            if let Some(checkpoints) = &self.checkpoints
                && let Ok(mut checkpoints) = checkpoints.lock()
            {
                checkpoints.push(context.checkpoint());
            }
            if let Ok(mut results) = self.results.lock() {
                results.push(result);
            }
        }
        JobOutcome {
            counts: JobCounts {
                completed: 1,
                ..Default::default()
            },
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

fn scripted_body(
    records: Vec<ItemHistoryRecord>,
    checkpoints: Option<Arc<Mutex<Vec<Checkpoint>>>>,
) -> (Arc<dyn JobBody>, RecordResults) {
    let results = Arc::new(Mutex::new(Vec::new()));
    (
        Arc::new(ScriptedBody {
            records,
            results: results.clone(),
            checkpoints,
        }),
        results,
    )
}

#[test]
fn invalid_items_do_not_write_or_make_the_context_continue_impossible()
-> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(ItemHistoryStore::default());
    let runtime = make_runtime(store.clone())?;
    runtime.register(descriptor(HistoryMode::PerItem))?;

    let mut invalid = Vec::new();
    let mut empty_ref = item(0);
    empty_ref.item_ref.clear();
    invalid.push(empty_ref);
    let mut long_ref = item(0);
    long_ref.item_ref = vec![0; 1_025];
    invalid.push(long_ref);
    let mut long_location = item(0);
    long_location.location_text = "x".repeat(32_769);
    invalid.push(long_location);
    let mut invalid_displacement = item(0);
    invalid_displacement.displaced = Some(du_core_contracts::job::DisplacedItem {
        trash_ref: None,
        name: None,
        original_location: None,
        trashed_at_ms: None,
    });
    invalid.push(invalid_displacement);
    let mut empty_displaced_name = item(0);
    empty_displaced_name.outcome = HistoryItemOutcome::Failed;
    empty_displaced_name.displaced = Some(du_core_contracts::job::DisplacedItem {
        trash_ref: None,
        name: Some(String::new()),
        original_location: None,
        trashed_at_ms: None,
    });
    invalid.push(empty_displaced_name);
    let mut empty_original_location = item(0);
    empty_original_location.outcome = HistoryItemOutcome::Failed;
    empty_original_location.displaced = Some(du_core_contracts::job::DisplacedItem {
        trash_ref: None,
        name: None,
        original_location: Some(String::new()),
        trashed_at_ms: None,
    });
    invalid.push(empty_original_location);
    let mut missing_overwrite = item(0);
    missing_overwrite.outcome = HistoryItemOutcome::Overwritten;
    invalid.push(missing_overwrite);
    let mut invalid_size = item(0);
    invalid_size.observed_size = ObservedSize::Exact(u64::MAX);
    invalid.push(invalid_size);
    let mut invalid_sequence = item(0);
    invalid_sequence.item_seq = i64::MAX as u64 + 1;
    invalid.push(invalid_sequence);
    invalid.push(item(0));

    let checkpoints = Arc::new(Mutex::new(Vec::new()));
    let (body, results) = scripted_body(invalid, Some(checkpoints.clone()));
    runtime.submit(submission(body)?)?;
    runtime.run_next()?;
    runtime.flush()?;

    let results = results.lock().map_err(|_| "results lock")?;
    assert_eq!(results.len(), 10);
    assert!(
        results[..9]
            .iter()
            .all(|result| *result == Err(ItemHistoryError::Invalid))
    );
    assert_eq!(results[9], Ok(()));
    assert_eq!(
        *checkpoints.lock().map_err(|_| "checkpoint lock")?,
        vec![Checkpoint::Continue; 10]
    );
    assert_eq!(
        store.items.lock().map_err(|_| "item lock")?.as_slice(),
        &[item(0)]
    );
    Ok(())
}

#[test]
fn identical_latest_item_resend_is_idempotent_and_conflicts_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(ItemHistoryStore::default());
    let runtime = make_runtime(store.clone())?;
    runtime.register(descriptor(HistoryMode::PerItem))?;

    let first = item(0);
    let mut conflicting = first.clone();
    conflicting.location_text = "different item content".into();
    let (body, results) = scripted_body(
        vec![first.clone(), first, conflicting, item(2), item(1)],
        None,
    );
    runtime.submit(submission(body)?)?;
    runtime.run_next()?;
    runtime.flush()?;

    assert_eq!(
        *results.lock().map_err(|_| "results lock")?,
        [
            Ok(()),
            Ok(()),
            Err(ItemHistoryError::Duplicate),
            Err(ItemHistoryError::OutOfOrder),
            Ok(())
        ]
    );
    assert_eq!(
        store
            .items
            .lock()
            .map_err(|_| "item lock")?
            .iter()
            .map(|item| item.item_seq)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    Ok(())
}
