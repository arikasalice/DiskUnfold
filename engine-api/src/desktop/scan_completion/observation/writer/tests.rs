use super::*;
use du_core_contracts::{
    Id, SizeEvidence,
    result::ResultCompleteness,
    scope_observation_log::{
        ObservedAmount, ObservedSize, SCOPE_FINGERPRINT_FORMAT_V1, ScopeObservationAppend,
        ScopeObservationOrigin,
    },
};
use std::{
    collections::VecDeque,
    sync::{
        Condvar,
        atomic::{AtomicUsize, Ordering},
    },
};

const LONG: Duration = Duration::from_secs(30);

fn row(n: u8) -> ScopeObservation {
    let size = |bytes| ObservedSize {
        amount: ObservedAmount::Exact(bytes),
        evidence: SizeEvidence::Measured,
    };
    ScopeObservation {
        observation_id: Id::from_v7_bytes([n.wrapping_add(100); 16]),
        scope_fingerprint: [7; 32],
        fingerprint_format: SCOPE_FINGERPRINT_FORMAT_V1,
        observed_at_ms: 1_000 + i64::from(n),
        job_id: Id::from_v7_bytes([n; 16]),
        origin: ScopeObservationOrigin::Scan,
        completeness: ResultCompleteness::Complete,
        size_rule_id: "size.agg.subtree-unique".into(),
        size_rule_revision: 1,
        logical: size(10),
        allocated: size(20),
        unique_physical: size(5),
        created_at_ms: 2_000,
    }
}

fn job(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}

/// Holds every append until it is opened, like a database another writer has locked.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        while !*open {
            open = self
                .changed
                .wait(open)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn open(&self) {
        *self.open.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }
}

#[derive(Default)]
struct Log {
    stored: Mutex<Vec<Id>>,
    calls: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
    scripted: Mutex<VecDeque<ScopeObservationWriteError>>,
    always: Mutex<Option<ScopeObservationWriteError>>,
    panic_once: AtomicBool,
    gate: Option<Arc<Gate>>,
}

impl Log {
    fn gated() -> (Arc<Self>, Arc<Gate>) {
        let gate = Arc::new(Gate::default());
        let log = Arc::new(Self {
            gate: Some(gate.clone()),
            ..Self::default()
        });
        (log, gate)
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn stored(&self) -> Vec<Id> {
        self.stored
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn fail_next(&self, errors: &[ScopeObservationWriteError]) {
        self.scripted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(errors.iter().copied());
    }

    fn fail_always(&self, error: Option<ScopeObservationWriteError>) {
        *self.always.lock().unwrap_or_else(PoisonError::into_inner) = error;
    }

    /// Waits until the worker has made `count` calls.
    fn wait_for_calls(&self, count: usize) -> TestResult {
        let deadline = Instant::now() + LONG;
        while self.calls() < count {
            if Instant::now() > deadline {
                return Err("the worker made no call".into());
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
}

impl ScopeObservationLog for Log {
    fn append(
        &self,
        observation: &ScopeObservation,
    ) -> Result<ScopeObservationAppend, ScopeObservationWriteError> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = self.attempt(observation);
        self.active.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

impl Log {
    fn attempt(
        &self,
        observation: &ScopeObservation,
    ) -> Result<ScopeObservationAppend, ScopeObservationWriteError> {
        if let Some(gate) = &self.gate {
            gate.wait();
        }
        if self.panic_once.swap(false, Ordering::SeqCst) {
            std::panic::resume_unwind(Box::new("the storage adapter failed"));
        }
        let scripted = self
            .scripted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        let always = *self.always.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(error) = scripted.or(always) {
            return Err(error);
        }
        self.stored
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(observation.job_id.clone());
        Ok(ScopeObservationAppend::Added)
    }
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn policy(pauses: usize, drain: Duration, first_wait: Duration) -> Policy {
    Policy {
        busy_pauses: vec![Duration::ZERO; pauses],
        drain_after_close: drain,
        first_close_wait: first_wait,
    }
}

fn writer(log: &Arc<Log>, policy: Policy) -> io::Result<ObservationWriter> {
    let log: Arc<dyn ScopeObservationLog> = log.clone();
    ObservationWriter::start_with(log, policy)
}

#[test]
fn the_production_timing_is_bounded() {
    let production = Policy::default();
    assert_eq!(QUEUE_CAPACITY, 64);
    assert_eq!(production.busy_pauses.len(), 2);
    // One row cannot keep the worker for more than three busy timeouts plus its pauses.
    let pauses: Duration = production.busy_pauses.iter().sum();
    assert!(pauses <= Duration::from_secs(5));
    assert!(production.first_close_wait <= Duration::from_secs(1));
    assert!(production.drain_after_close <= Duration::from_secs(2));
}

#[test]
fn rows_are_stored_one_at_a_time_in_submission_order() -> TestResult {
    let log = Arc::new(Log::default());
    let writer = writer(&log, policy(0, LONG, LONG))?;
    for n in 1..=20 {
        writer.submit(row(n)).map_err(|_| "queue refused a row")?;
    }
    writer.flush();
    assert_eq!(log.stored(), (1..=20).map(job).collect::<Vec<_>>());
    assert_eq!(log.max_active.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn a_blocked_log_never_blocks_the_submitter_and_a_full_queue_drops_the_new_row() -> TestResult {
    let (log, gate) = Log::gated();
    let writer = writer(&log, policy(0, LONG, LONG))?;
    let started = Instant::now();
    writer.submit(row(1)).map_err(|_| "first row refused")?;
    // The worker now holds row 1 inside the blocked append.
    log.wait_for_calls(1)?;
    for n in 0..QUEUE_CAPACITY {
        let n = u8::try_from(n)? + 2;
        writer.submit(row(n)).map_err(|_| "row refused early")?;
    }
    let dropped = writer.submit(row(200));
    assert_eq!(dropped, Err(DropReason::QueueFull));
    assert_eq!(DropReason::QueueFull.reason(), "queue_full");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(log.stored().is_empty());
    gate.open();
    writer.flush();
    let stored = log.stored();
    assert_eq!(stored.len(), QUEUE_CAPACITY + 1);
    assert!(!stored.contains(&job(200)));
    // The queue accepts rows again once the worker has caught up.
    writer.submit(row(201)).map_err(|_| "queue stayed full")?;
    writer.flush();
    assert_eq!(log.stored().last(), Some(&job(201)));
    Ok(())
}

#[test]
fn a_busy_write_is_retried_on_the_worker_until_it_succeeds() -> TestResult {
    let log = Arc::new(Log::default());
    log.fail_next(&[
        ScopeObservationWriteError::Busy,
        ScopeObservationWriteError::Busy,
    ]);
    let writer = writer(&log, policy(2, LONG, LONG))?;
    writer.submit(row(1)).map_err(|_| "row refused")?;
    writer.flush();
    assert_eq!((log.calls(), log.stored()), (3, vec![job(1)]));
    Ok(())
}

#[test]
fn a_database_that_stays_busy_ends_the_row_after_its_attempts_and_the_next_row_proceeds()
-> TestResult {
    let log = Arc::new(Log::default());
    log.fail_always(Some(ScopeObservationWriteError::Busy));
    let writer = writer(&log, policy(2, LONG, LONG))?;
    writer.submit(row(1)).map_err(|_| "row refused")?;
    writer.flush();
    // The first attempt and two retries; none of them stored anything.
    assert_eq!((log.calls(), log.stored().len()), (3, 0));
    log.fail_always(None);
    writer.submit(row(2)).map_err(|_| "row refused")?;
    writer.flush();
    assert_eq!(log.stored(), vec![job(2)]);
    Ok(())
}

#[test]
fn only_a_busy_answer_is_retried() -> TestResult {
    for error in [
        ScopeObservationWriteError::Invalid,
        ScopeObservationWriteError::DiskFull,
        ScopeObservationWriteError::Storage,
    ] {
        let log = Arc::new(Log::default());
        log.fail_next(&[error]);
        let writer = writer(&log, policy(2, LONG, LONG))?;
        writer.submit(row(1)).map_err(|_| "row refused")?;
        writer.flush();
        assert_eq!((log.calls(), log.stored().len()), (1, 0), "{error:?}");
    }
    Ok(())
}

#[test]
fn a_panicking_log_loses_one_row_and_the_worker_goes_on() -> TestResult {
    let log = Arc::new(Log::default());
    log.panic_once.store(true, Ordering::SeqCst);
    let writer = writer(&log, policy(2, LONG, LONG))?;
    writer.submit(row(1)).map_err(|_| "row refused")?;
    writer.submit(row(2)).map_err(|_| "row refused")?;
    writer.flush();
    assert_eq!(log.stored(), vec![job(2)]);
    Ok(())
}

#[test]
fn closing_writes_the_queued_rows_then_refuses_new_ones() -> TestResult {
    let (log, gate) = Log::gated();
    let writer = writer(&log, policy(0, LONG, LONG))?;
    for n in 1..=4 {
        writer.submit(row(n)).map_err(|_| "row refused")?;
    }
    log.wait_for_calls(1)?;
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        gate.open();
    });
    writer.close()?;
    release.join().map_err(|_| "release thread panicked")?;
    assert_eq!(log.stored(), (1..=4).map(job).collect::<Vec<_>>());
    assert_eq!(writer.submit(row(5)), Err(DropReason::Closed));
    assert_eq!(DropReason::Closed.reason(), "writer_closed");
    // Closing again, as the shutdown sequence may, changes nothing.
    writer.close()?;
    assert_eq!(log.stored().len(), 4);
    Ok(())
}

#[test]
fn closing_an_idle_writer_returns_at_once_and_ends_the_worker() -> TestResult {
    let log = Arc::new(Log::default());
    // The first close may wait a whole minute for the worker. An idle worker ends as soon
    // as the queue closes, so a close that used up that wait would take three times the
    // bound below; the bound is far above any scheduling delay.
    let writer = writer(&log, policy(0, LONG, Duration::from_secs(60)))?;
    let started = Instant::now();
    writer.close()?;
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");
    Ok(())
}

#[test]
fn rows_still_queued_after_the_drain_limit_are_dropped_and_close_reports_the_write_in_progress()
-> TestResult {
    let (log, gate) = Log::gated();
    let writer = writer(&log, policy(0, Duration::ZERO, Duration::from_millis(20)))?;
    for n in 1..=4 {
        writer.submit(row(n)).map_err(|_| "row refused")?;
    }
    log.wait_for_calls(1)?;
    // The worker is still inside the first write, so the first close cannot finish yet.
    assert!(writer.close().is_err());
    assert!(writer.close().is_err());
    gate.open();
    let deadline = Instant::now() + LONG;
    while writer.close().is_err() {
        assert!(Instant::now() < deadline, "the worker never ended");
        thread::sleep(Duration::from_millis(1));
    }
    // The write that was under way finished; the rows behind it were dropped.
    assert_eq!(log.stored(), vec![job(1)]);
    assert_eq!(log.calls(), 1);
    Ok(())
}

#[test]
fn closing_ends_a_retry_pause_instead_of_waiting_for_it() -> TestResult {
    let log = Arc::new(Log::default());
    log.fail_always(Some(ScopeObservationWriteError::Busy));
    let writer = ObservationWriter::start_with(
        log.clone(),
        Policy {
            busy_pauses: vec![Duration::from_secs(600)],
            drain_after_close: Duration::ZERO,
            first_close_wait: LONG,
        },
    )?;
    writer.submit(row(1)).map_err(|_| "row refused")?;
    log.wait_for_calls(1)?;
    let started = Instant::now();
    writer.close()?;
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!((log.calls(), log.stored().len()), (1, 0));
    Ok(())
}
