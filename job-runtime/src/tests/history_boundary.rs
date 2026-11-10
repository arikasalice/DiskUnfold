use super::*;

struct AppendResult {
    result: Result<HistoryAppend, String>,
    calls: AtomicUsize,
}

impl JobHistory for AppendResult {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.clone()
    }
}

#[test]
fn terminal_delivery_preserves_recording_boundaries() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            HistoryMode::Summary,
            false,
            Ok(HistoryAppend::Added),
            1,
            true,
            false,
        ),
        (
            HistoryMode::Summary,
            false,
            Ok(HistoryAppend::Duplicate),
            1,
            true,
            false,
        ),
        (HistoryMode::Summary, false, Err("db.busy"), 1, false, true),
        (
            HistoryMode::None,
            false,
            Ok(HistoryAppend::Added),
            0,
            false,
            false,
        ),
        (
            HistoryMode::Summary,
            true,
            Ok(HistoryAppend::Added),
            0,
            false,
            false,
        ),
    ];
    for (mode, sample, result, calls, recorded, warned) in cases {
        let history = Arc::new(AppendResult {
            result: result.map_err(str::to_owned),
            calls: AtomicUsize::new(0),
        });
        let events = Arc::new(Events::default());
        let runtime = JobRuntime::new(
            Arc::new(Identity(Id::parse("00000000-0000-7000-8000-000000000001")?)),
            Arc::new(Clock),
            history.clone(),
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
        assert_eq!(runtime.state(&id)?, JobState::Done);
        assert_eq!(history.calls.load(Ordering::SeqCst), calls);
        let observed = events.0.lock().map_err(|_| "events poisoned")?;
        assert!(matches!(observed.as_slice(), [..,
            JobEvent::StateChanged { job_id, state: JobState::Done, sequence },
            JobEvent::Finished { summary, sequence: terminal_seq, history_recorded, warning }
        ] if *job_id == id && summary.job_id == id && *terminal_seq == *sequence + 1
            && *history_recorded == recorded && warning.is_some() == warned));
    }
    Ok(())
}
