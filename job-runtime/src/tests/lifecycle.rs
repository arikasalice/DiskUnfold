use super::*;
use std::sync::atomic::AtomicI64;
struct MutableClock(AtomicI64);
impl JobClock for MutableClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
#[test]
fn terminal_body_is_released_and_entry_expires_after_ten_minutes()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let clock = Arc::new(MutableClock(AtomicI64::new(0)));
    let runtime = JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        clock.clone(),
        Arc::new(History::default()),
        Arc::new(Events::default()),
    )?;
    runtime.register(descriptor())?;
    let body = Arc::new(InstantBody);
    let weak = Arc::downgrade(&body);
    let id = created(runtime.submit(sample_submission(body.clone())?)?)?;
    let queued = runtime.snapshot(&id)?;
    assert_eq!(queued.state, JobState::Queued);
    assert_eq!(queued.sequence, 1);
    drop(body);
    assert!(weak.upgrade().is_some());
    runtime.cancel(&id)?;
    assert!(weak.upgrade().is_none());
    clock.0.store(599_999, Ordering::SeqCst);
    assert_eq!(runtime.state(&id)?, JobState::Cancelled);
    let snapshot = runtime.snapshot(&id)?;
    assert_eq!(snapshot.state, JobState::Cancelled);
    assert_eq!(snapshot.kind_id, "scan.metadata");
    assert!(snapshot.sequence > queued.sequence);
    assert_eq!(snapshot.result_version_id, None);
    clock.0.store(600_000, Ordering::SeqCst);
    assert_eq!(runtime.state(&id), Err(RuntimeError::NotFound));
    assert_eq!(runtime.snapshot(&id), Err(RuntimeError::NotFound));
    let body = Arc::new(InstantBody);
    let weak = Arc::downgrade(&body);
    let second = created(runtime.submit(sample_submission(body.clone())?)?)?;
    drop(body);
    assert_eq!(runtime.run_next()?, Some(second));
    assert!(weak.upgrade().is_none());
    Ok(())
}
#[test]
fn bounded_backlog_reserves_terminal_delivery() -> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let history = Arc::new(FirstBlockingHistory {
        entered: entered.clone(),
        release: release.clone(),
        first: AtomicBool::new(true),
    });
    let runtime = JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        history,
        Arc::new(Events::default()),
    )?;
    runtime.register(descriptor())?;
    let first = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    runtime.cancel(&first)?;
    entered.wait();
    for _ in 1..64 {
        let id = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
        runtime.cancel(&id)?;
    }
    let busy = runtime.submit(sample_submission(Arc::new(InstantBody))?);
    release.wait();
    runtime.flush()?;
    assert_eq!(busy, Err(RuntimeError::DeliveryBusy));
    created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    Ok(())
}

struct FirstBlockingHistory {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    first: AtomicBool,
}
impl JobHistory for FirstBlockingHistory {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        if self.first.swap(false, Ordering::SeqCst) {
            self.entered.wait();
            self.release.wait();
        }
        Ok(HistoryAppend::Added)
    }
}

#[test]
fn shutdown_ticket_times_out_without_blocking_and_later_completes()
-> Result<(), Box<dyn std::error::Error>> {
    let fallback = Id::parse("00000000-0000-7000-8000-000000000001")?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let history = Arc::new(FirstBlockingHistory {
        entered: entered.clone(),
        release: release.clone(),
        first: AtomicBool::new(true),
    });
    let runtime = JobRuntime::new(
        Arc::new(SequentialIdentity {
            next: AtomicUsize::new(1),
            fallback,
        }),
        Arc::new(Clock),
        history,
        Arc::new(Events::default()),
    )?;
    runtime.register(descriptor())?;
    let id = created(runtime.submit(sample_submission(Arc::new(InstantBody))?)?)?;
    assert!(matches!(
        runtime.begin_shutdown(std::time::Duration::ZERO),
        Err(RuntimeError::ActiveJobs)
    ));
    runtime.cancel(&id)?;
    entered.wait();
    let ticket = runtime.begin_shutdown(std::time::Duration::ZERO)?;
    assert_eq!(ticket.poll(), ShutdownStatus::TimedOut);
    release.wait();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if ticket.poll() == ShutdownStatus::Complete {
            assert_eq!(ticket.poll(), ShutdownStatus::Complete);
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err("shutdown did not complete".into());
        }
        std::thread::yield_now();
    }
    assert!(matches!(
        runtime.submit(sample_submission(Arc::new(InstantBody))?),
        Err(RuntimeError::ShuttingDown)
    ));
    Ok(())
}
