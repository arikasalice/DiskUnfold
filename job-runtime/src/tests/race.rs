use super::*;

struct RacingBody {
    entered: Arc<Barrier>,
    race: Arc<Barrier>,
}
impl JobBody for RacingBody {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }
    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        self.entered.wait();
        self.race.wait();
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
#[test]
fn cancellation_and_completion_race_produces_one_terminal() -> Result<(), Box<dyn std::error::Error>>
{
    for _ in 0..30 {
        let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
        let history = Arc::new(History::default());
        let events = Arc::new(Events::default());
        let runtime = Arc::new(JobRuntime::new(
            Arc::new(SequentialIdentity {
                next: AtomicUsize::new(1),
                fallback,
            }),
            Arc::new(Clock),
            history.clone(),
            events.clone(),
        )?);
        runtime.register(descriptor())?;
        let entered = Arc::new(Barrier::new(2));
        let race = Arc::new(Barrier::new(3));
        let id = created(runtime.submit(sample_submission(Arc::new(RacingBody {
            entered: entered.clone(),
            race: race.clone(),
        }))?)?)?;
        let worker_runtime = runtime.clone();
        let worker = std::thread::spawn(move || worker_runtime.run_next());
        entered.wait();
        let cancel_runtime = runtime.clone();
        let cancel_id = id.clone();
        let cancel_race = race.clone();
        let cancel = std::thread::spawn(move || {
            cancel_race.wait();
            cancel_runtime.cancel(&cancel_id)
        });
        race.wait();
        worker.join().map_err(|_| "worker panicked")??;
        cancel.join().map_err(|_| "cancel panicked")??;
        runtime.flush()?;
        assert!(matches!(
            runtime.state(&id)?,
            JobState::Done | JobState::Cancelled
        ));
        assert_eq!(history.calls.load(Ordering::SeqCst), 1);
        let events = events.0.lock().map_err(|_| "events poisoned")?;
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, JobEvent::Finished { .. }))
                .count(),
            1
        );
    }
    Ok(())
}

struct ReportingBody {
    sender: std::sync::mpsc::Sender<Box<dyn du_core_contracts::job::JobFlow>>,
}

impl JobBody for ReportingBody {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        if let Ok(flow) = context.try_start_flow() {
            let _ = self.sender.send(flow);
        }
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

#[test]
fn concurrent_jobs_keep_independent_sequences_and_one_final_event()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let events = Arc::new(Events::default());
    let runtime = Arc::new(JobRuntime::new_with_limits(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        events.clone(),
        FlowLimits {
            metadata_io: 2,
            queued_items: 8,
            item_bytes: 32,
        },
    )?);
    runtime.register(descriptor())?;
    let (first_tx, first_rx) = std::sync::mpsc::channel();
    let (second_tx, second_rx) = std::sync::mpsc::channel();
    let first = created(runtime.submit(sample_submission(Arc::new(ReportingBody {
        sender: first_tx,
    }))?)?)?;
    let second = created(runtime.submit(sample_submission(Arc::new(ReportingBody {
        sender: second_tx,
    }))?)?)?;
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let runtime = runtime.clone();
            std::thread::spawn(move || runtime.run_next())
        })
        .collect();
    let first_flow = first_rx.recv_timeout(std::time::Duration::from_secs(2))?;
    let second_flow = second_rx.recv_timeout(std::time::Duration::from_secs(2))?;
    first_flow
        .report_item(b"first-1")
        .map_err(|e| format!("{e:?}"))?;
    second_flow
        .report_item(b"second")
        .map_err(|e| format!("{e:?}"))?;
    first_flow
        .report_item(b"first-2")
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        runtime.cancel(&first)?,
        CancelResult::Accepted(JobState::CancelRequested)
    ));
    first_flow.finish(JobCounts::default());
    second_flow.finish(JobCounts::default());
    for worker in workers {
        worker.join().map_err(|_| "worker panicked")??;
    }
    assert_eq!(
        runtime.cancel(&first)?,
        CancelResult::AlreadyTerminal(JobState::Cancelled)
    );
    assert_eq!(
        runtime.cancel(&second)?,
        CancelResult::AlreadyTerminal(JobState::Done)
    );
    runtime.flush()?;
    let events = events.0.lock().map_err(|_| "events poisoned")?;
    for (id, expected_items) in [(&first, 2), (&second, 1)] {
        assert_eq!(
            events
                .iter()
                .filter(
                    |event| matches!(event, JobEvent::ItemResult { job_id, .. } if job_id == id)
                )
                .count(),
            expected_items
        );
        let observed: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                JobEvent::StateChanged {
                    job_id, sequence, ..
                }
                | JobEvent::ItemResult {
                    job_id, sequence, ..
                } if job_id == id => Some((*sequence, false)),
                JobEvent::Finished {
                    summary, sequence, ..
                } if &summary.job_id == id => Some((*sequence, true)),
                _ => None,
            })
            .collect();
        assert!(observed.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(observed.iter().filter(|(_, finished)| *finished).count(), 1);
        assert_eq!(observed.last().map(|(_, finished)| *finished), Some(true));
        assert_eq!(
            runtime.snapshot(id)?.sequence,
            observed.last().ok_or("missing event")?.0
        );
    }
    Ok(())
}

#[test]
fn terminal_gate_rejects_late_flow_reports_without_advancing_sequence()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let events = Arc::new(Events::default());
    let runtime = Arc::new(JobRuntime::new_with_limits(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        Arc::new(History::default()),
        events.clone(),
        FlowLimits {
            metadata_io: 1,
            queued_items: 2,
            item_bytes: 32,
        },
    )?);
    runtime.register(descriptor())?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let id = created(runtime.submit(sample_submission(Arc::new(ReportingBody { sender }))?)?)?;
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    let flow = receiver.recv_timeout(std::time::Duration::from_secs(2))?;
    flow.report_item(b"before").map_err(|e| format!("{e:?}"))?;

    // Exercise the internal terminal gate with a still-held flow. Normal run_next
    // waits for registered flows before it commits the terminal outcome.
    let terminal_sequence = runtime.finish(
        &id,
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        },
    )?;
    runtime.flush()?;
    let confirmed = runtime.snapshot(&id)?;
    assert!(confirmed.terminal.is_some());
    assert_eq!(
        flow.report_item(b"late"),
        Err(du_core_contracts::job::JobFlowError::Closed)
    );
    flow.finish(JobCounts::default());
    worker.join().map_err(|_| "worker panicked")??;
    runtime.flush()?;
    assert_eq!(runtime.snapshot(&id)?, confirmed);
    assert_eq!(confirmed.sequence, terminal_sequence);
    let events = events.0.lock().map_err(|_| "events poisoned")?;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, JobEvent::Finished { .. }))
            .count(),
        1
    );
    assert!(
        matches!(events.last(), Some(JobEvent::Finished { sequence, .. }) if *sequence == terminal_sequence)
    );
    assert!(
        !events.iter().any(
            |event| matches!(event, JobEvent::ItemResult { payload, .. } if payload == b"late")
        )
    );
    Ok(())
}
