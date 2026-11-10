use super::*;
use du_core_contracts::job::{
    HistoryAppendOutcome, HistoryAppendReceipt, HistoryChangeReason, HistoryChanged,
    HistoryRevision,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Delivered {
    Job(JobEvent),
    History(HistoryChanged),
}

#[derive(Default)]
struct Collector(Mutex<Vec<Delivered>>);
impl JobEvents for Collector {
    fn publish(&self, event: JobEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(Delivered::Job(event));
        }
    }
    fn publish_history_changed(&self, event: HistoryChanged) {
        if let Ok(mut events) = self.0.lock() {
            events.push(Delivered::History(event));
        }
    }
}

struct ReceiptStore {
    result: Result<HistoryAppendOutcome, String>,
    calls: AtomicUsize,
}
impl JobHistory for ReceiptStore {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Err("unexpected legacy append".into())
    }
    fn append_with_receipt(&self, _: &JobSummary) -> Result<HistoryAppendOutcome, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.clone()
    }
}

#[test]
fn receipt_drives_only_new_history_notification_after_terminal_events()
-> Result<(), Box<dyn std::error::Error>> {
    let receipt = HistoryAppendReceipt {
        revision: HistoryRevision {
            delete_generation: 3,
            max_record_seq: 71,
        },
        observed_at_ms: 3456,
    };
    let cases = [
        (
            HistoryMode::Summary,
            false,
            Ok(HistoryAppendOutcome::Added(Some(receipt))),
            1,
            true,
        ),
        (
            HistoryMode::Summary,
            false,
            Ok(HistoryAppendOutcome::Added(None)),
            1,
            false,
        ),
        (
            HistoryMode::Summary,
            false,
            Ok(HistoryAppendOutcome::Duplicate),
            1,
            false,
        ),
        (HistoryMode::Summary, false, Err("storage.failed"), 1, false),
        (
            HistoryMode::None,
            false,
            Ok(HistoryAppendOutcome::Added(Some(receipt))),
            0,
            false,
        ),
        (
            HistoryMode::Summary,
            true,
            Ok(HistoryAppendOutcome::Added(Some(receipt))),
            0,
            false,
        ),
    ];
    for (mode, sample, result, calls, notify) in cases {
        let store = Arc::new(ReceiptStore {
            result: result.map_err(str::to_owned),
            calls: AtomicUsize::new(0),
        });
        let events = Arc::new(Collector::default());
        let runtime = JobRuntime::new(
            Arc::new(Identity(Id::parse("00000000-0000-7000-8000-000000000001")?)),
            Arc::new(Clock),
            store.clone(),
            events.clone(),
        )?;
        let mut kind = descriptor();
        kind.history = mode;
        runtime.register(kind)?;
        let mut submission = sample_submission(Arc::new(InstantBody))?;
        submission.sample = sample;
        let id = created(runtime.submit(submission)?)?;
        assert_eq!(runtime.run_next()?, Some(id.clone()));
        runtime.flush()?;
        assert_eq!(store.calls.load(Ordering::SeqCst), calls);
        let observed = events.0.lock().map_err(|_| "events poisoned")?;
        let terminal = observed
            .iter()
            .position(|event| matches!(event, Delivered::Job(JobEvent::Finished { .. })))
            .ok_or("missing terminal")?;
        assert!(
            matches!(observed.get(terminal.wrapping_sub(1)), Some(Delivered::Job(JobEvent::StateChanged { job_id, state: JobState::Done, .. })) if *job_id == id)
        );
        let changes = observed
            .iter()
            .filter(|event| matches!(event, Delivered::History(_)))
            .count();
        assert_eq!(changes, usize::from(notify));
        if notify {
            assert_eq!(
                observed.get(terminal + 1),
                Some(&Delivered::History(HistoryChanged {
                    revision: receipt.revision,
                    reason: HistoryChangeReason::Appended,
                    count: 1,
                    observed_at_ms: receipt.observed_at_ms,
                    deleted_job_ids: Vec::new(),
                }))
            );
        }
    }
    Ok(())
}

#[test]
fn legacy_store_returns_no_receipt() -> Result<(), Box<dyn std::error::Error>> {
    let store = History::default();
    let summary = JobSummary {
        job_id: Id::parse("00000000-0000-7000-8000-000000000001")?,
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "summary".into(),
        state: JobState::Done,
        counts: JobCounts::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(1),
        finished_at_ms: 2,
        correlation_id: Id::parse("00000000-0000-7000-8000-000000000002")?,
    };
    assert_eq!(
        store.append_with_receipt(&summary),
        Ok(HistoryAppendOutcome::Added(None))
    );
    let events = Arc::new(Collector::default());
    let runtime = JobRuntime::new(
        Arc::new(Identity(summary.job_id.clone())),
        Arc::new(Clock),
        Arc::new(store),
        events.clone(),
    )?;
    runtime.register(descriptor())?;
    created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    runtime.run_next()?;
    runtime.flush()?;
    let observed = events.0.lock().map_err(|_| "events poisoned")?;
    assert!(matches!(
        observed.last(),
        Some(Delivered::Job(JobEvent::Finished {
            history_recorded: true,
            ..
        }))
    ));
    assert!(
        !observed
            .iter()
            .any(|event| matches!(event, Delivered::History(_)))
    );
    Ok(())
}
