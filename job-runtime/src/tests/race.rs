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
