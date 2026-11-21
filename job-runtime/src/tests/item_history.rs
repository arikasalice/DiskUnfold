use super::*;
use du_core_contracts::{
    history_ipc::HistoryItemOutcome,
    job::{ItemHistoryError, ItemHistoryRecord, ItemHistoryStart, ObservedSize},
};

#[derive(Default)]
struct ItemHistoryStore {
    starts: Mutex<Vec<ItemHistoryStart>>,
    items: Mutex<Vec<ItemHistoryRecord>>,
    summaries: Mutex<Vec<JobSummary>>,
    fail_start: bool,
    fail_sequence: Option<u64>,
}

impl JobHistory for ItemHistoryStore {
    fn append(&self, summary: &JobSummary) -> Result<HistoryAppend, String> {
        self.summaries
            .lock()
            .map_err(|_| "summary lock".to_owned())?
            .push(summary.clone());
        Ok(HistoryAppend::Added)
    }

    fn append_item_start(&self, start: &ItemHistoryStart) -> Result<(), ItemHistoryError> {
        if self.fail_start {
            return Err(ItemHistoryError::Unavailable);
        }
        self.starts
            .lock()
            .map_err(|_| ItemHistoryError::Unavailable)?
            .push(start.clone());
        Ok(())
    }

    fn append_item_result(&self, _: &Id, item: &ItemHistoryRecord) -> Result<(), ItemHistoryError> {
        if self.fail_sequence == Some(item.item_seq) {
            return Err(ItemHistoryError::Busy);
        }
        self.items
            .lock()
            .map_err(|_| ItemHistoryError::Unavailable)?
            .push(item.clone());
        Ok(())
    }
}

struct PerItemBody {
    calls: Vec<u64>,
    results: RecordResults,
    counts: JobCounts,
    entered: Arc<AtomicBool>,
}

impl JobBody for PerItemBody {
    fn kind_id(&self) -> &str {
        "change.trash"
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        self.entered.store(true, Ordering::SeqCst);
        for sequence in &self.calls {
            if let Ok(mut results) = self.results.lock() {
                let result = context.record_item(item(*sequence));
                let stop = matches!(
                    result,
                    Err(ItemHistoryError::Busy | ItemHistoryError::Unavailable)
                );
                results.push(result);
                if stop {
                    break;
                }
            }
        }
        JobOutcome {
            counts: self.counts.clone(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

fn item(sequence: u64) -> ItemHistoryRecord {
    ItemHistoryRecord {
        item_seq: sequence,
        item_ref: vec![sequence as u8 + 1],
        location_raw: vec![b'/', sequence as u8 + b'1'],
        location_text: format!("item-{}", sequence + 1),
        outcome: HistoryItemOutcome::Completed,
        reason_key: None,
        review_needed: false,
        held: false,
        outcome_unknown: false,
        observed_size: ObservedSize::Exact(12),
        displaced: None,
    }
}

fn descriptor(history: HistoryMode) -> JobDescriptor {
    let mut descriptor = super::descriptor();
    descriptor.kind_id = "change.trash".into();
    descriptor.version = 1;
    descriptor.history = history;
    descriptor
}

fn submission(body: Arc<dyn JobBody>) -> Result<Submission, Box<dyn std::error::Error>> {
    Ok(Submission {
        kind_id: "change.trash".into(),
        deduplication_key: None,
        body,
        priority: JobPriority::Normal,
        scope_summary: "3 items".into(),
        scope_roots: Vec::new(),
        correlation_id: Id::parse("00000000-0000-7000-8000-000000000999")?,
        sample: false,
    })
}

fn make_runtime(history: Arc<ItemHistoryStore>) -> Result<JobRuntime, Box<dyn std::error::Error>> {
    let runtime = JobRuntime::new(
        Arc::new(Identity(Id::parse("00000000-0000-7000-8000-000000000001")?)),
        Arc::new(Clock),
        history,
        Arc::new(Events::default()),
    )?;
    Ok(runtime)
}

type RecordResults = Arc<Mutex<Vec<Result<(), ItemHistoryError>>>>;
type BodyFixture = (Arc<dyn JobBody>, RecordResults, Arc<AtomicBool>);

fn make_body(calls: Vec<u64>, counts: JobCounts) -> BodyFixture {
    let results = Arc::new(Mutex::new(Vec::new()));
    let entered = Arc::new(AtomicBool::new(false));
    let body: Arc<dyn JobBody> = Arc::new(PerItemBody {
        calls,
        results: results.clone(),
        counts,
        entered: entered.clone(),
    });
    (body, results, entered)
}

#[test]
fn per_item_start_results_and_zero_item_job_reach_terminal_history()
-> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(ItemHistoryStore::default());
    let runtime = make_runtime(store.clone())?;
    runtime.register(descriptor(HistoryMode::PerItem))?;
    let (body, _, _) = make_body(
        vec![0, 1, 2],
        JobCounts {
            completed: 3,
            ..Default::default()
        },
    );
    runtime.submit(submission(body)?)?;
    runtime.run_next()?;
    runtime.flush()?;
    assert_eq!(store.starts.lock().map_err(|_| "start lock")?.len(), 1);
    assert_eq!(
        store
            .items
            .lock()
            .map_err(|_| "item lock")?
            .iter()
            .map(|item| item.item_seq)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(store.summaries.lock().map_err(|_| "summary lock")?.len(), 1);

    let store = Arc::new(ItemHistoryStore::default());
    let runtime = make_runtime(store.clone())?;
    runtime.register(descriptor(HistoryMode::PerItem))?;
    let (body, _, _) = make_body(Vec::new(), JobCounts::default());
    runtime.submit(submission(body)?)?;
    runtime.run_next()?;
    runtime.flush()?;
    assert_eq!(store.starts.lock().map_err(|_| "start lock")?.len(), 1);
    assert!(store.items.lock().map_err(|_| "item lock")?.is_empty());
    assert_eq!(store.summaries.lock().map_err(|_| "summary lock")?.len(), 1);
    Ok(())
}

#[test]
fn order_errors_do_not_write_and_item_storage_failure_fails_the_job()
-> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(ItemHistoryStore::default());
    let runtime = make_runtime(store.clone())?;
    runtime.register(descriptor(HistoryMode::PerItem))?;
    let (body, results, _) = make_body(
        vec![1, 0, 0, 1],
        JobCounts {
            completed: 2,
            ..Default::default()
        },
    );
    runtime.submit(submission(body)?)?;
    runtime.run_next()?;
    runtime.flush()?;
    assert_eq!(
        *results.lock().map_err(|_| "results lock")?,
        [
            Err(ItemHistoryError::OutOfOrder),
            Ok(()),
            Err(ItemHistoryError::Duplicate),
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

    let store = Arc::new(ItemHistoryStore {
        fail_sequence: Some(1),
        ..Default::default()
    });
    let runtime = make_runtime(store.clone())?;
    runtime.register(descriptor(HistoryMode::PerItem))?;
    let counts = JobCounts {
        completed: 1,
        unprocessed: 2,
        ..Default::default()
    };
    let (body, results, _) = make_body(vec![0, 1, 2], counts.clone());
    let id = match runtime.submit(submission(body)?)? {
        SubmitResult::Created(id) => id,
        SubmitResult::Existing(_) => return Err("unexpected duplicate".into()),
    };
    runtime.run_next()?;
    runtime.flush()?;
    let summaries = store.summaries.lock().map_err(|_| "summary lock")?;
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].state, JobState::Partial);
    assert_eq!(
        summaries[0].failure_reason_key.as_deref(),
        Some("history.record_failed")
    );
    assert_eq!(summaries[0].counts, counts);
    assert_eq!(
        *results.lock().map_err(|_| "results lock")?,
        [Ok(()), Err(ItemHistoryError::Busy)]
    );
    assert_eq!(
        store
            .items
            .lock()
            .map_err(|_| "item lock")?
            .iter()
            .map(|item| item.item_seq)
            .collect::<Vec<_>>(),
        [0]
    );
    assert_eq!(runtime.state(&id)?, JobState::Partial);
    Ok(())
}

#[test]
fn failed_start_skips_body_and_summary_or_none_kinds_keep_existing_behavior()
-> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(ItemHistoryStore {
        fail_start: true,
        ..Default::default()
    });
    let runtime = make_runtime(store.clone())?;
    runtime.register(descriptor(HistoryMode::PerItem))?;
    let (body, _, entered) = make_body(Vec::new(), JobCounts::default());
    let id = match runtime.submit(submission(body)?)? {
        SubmitResult::Created(id) => id,
        SubmitResult::Existing(_) => return Err("unexpected duplicate".into()),
    };
    runtime.run_next()?;
    runtime.flush()?;
    assert!(!entered.load(Ordering::SeqCst));
    assert_eq!(runtime.state(&id)?, JobState::Failed);
    assert_eq!(
        runtime.snapshot(&id)?.failure_reason_key.as_deref(),
        Some("history.record_failed")
    );
    assert!(
        store
            .summaries
            .lock()
            .map_err(|_| "summary lock")?
            .is_empty()
    );

    for mode in [HistoryMode::Summary, HistoryMode::None] {
        let store = Arc::new(ItemHistoryStore::default());
        let runtime = make_runtime(store.clone())?;
        runtime.register(descriptor(mode))?;
        let (body, results, _) = make_body(vec![0], JobCounts::default());
        runtime.submit(submission(body)?)?;
        runtime.run_next()?;
        runtime.flush()?;
        assert_eq!(
            *results.lock().map_err(|_| "results lock")?,
            [Err(ItemHistoryError::Unavailable)]
        );
        assert!(store.starts.lock().map_err(|_| "start lock")?.is_empty());
        assert!(store.items.lock().map_err(|_| "item lock")?.is_empty());
        assert_eq!(
            store.summaries.lock().map_err(|_| "summary lock")?.len(),
            usize::from(mode == HistoryMode::Summary)
        );
    }
    Ok(())
}
