//! scan.prioritize during pause, during cancellation and racing the job's end
//! (FR8.HD1.app-host.DD1).
use super::{remote::*, *};

type Calls = Arc<Mutex<Vec<(Vec<Vec<u8>>, Call)>>>;

const SHAPE: Shape = Shape {
    depth: 1,
    dirs: 3,
    files: 1,
    batch: 8,
};

/// One root whose open of "d0" waits on `gate`; every call is recorded.
fn gated_probe(gate: &Arc<Gate>, calls: &Calls) -> Arc<Probe> {
    let (gate, calls) = (gate.clone(), calls.clone());
    Probe::new(
        SHAPE,
        1,
        Box::new(move |_, path, call| {
            calls
                .lock()
                .map_err(|_| io_error("test.record"))?
                .push((path.to_vec(), call));
            if call == Call::Open && path == [b"d0".to_vec()] {
                gate.wait()?;
            }
            Ok(())
        }),
    )
}

/// Collects `probe` on a thread with priority control and `limit` workers per lane.
pub(super) fn spawn_scan(
    probe: &Arc<Probe>,
    groups: Vec<usize>,
    workers: usize,
    limit: usize,
    context: Arc<dyn JobContext>,
) -> (
    ScanControlHandle,
    thread::JoinHandle<Result<Run, TestError>>,
) {
    let session = Arc::new(ScanControlSession::new(groups.len()));
    let handle = session.handle();
    let probe = probe.clone();
    let collector = thread::spawn(move || {
        let ids = Ids(AtomicUsize::new(1));
        let sink = IngestSink::default();
        let roots = (0..groups.len())
            .map(|index| observed_root(&format!("P:\\{index}"), index as u8 + 1))
            .collect();
        let build = sink.open_forest(ids.next_id(), roots);
        let devices = DevicePlan { groups, limit };
        let options = (devices, None, Some(&*session));
        let outcome = collect_open_forest_diagnostic(
            build,
            probe.roots(),
            &ids,
            &*context,
            workers,
            None,
            options,
        );
        drop(session);
        let version = sink
            .pin()
            .map_err(|error| format!("{error:?}"))?
            .ok_or("missing result")?;
        Ok(Run {
            outcome,
            completeness: version.completeness(),
            rows: rows(&version)?,
            issues: Vec::new(),
        })
    });
    (handle, collector)
}

fn d(name: &str) -> Vec<Vec<u8>> {
    vec![name.as_bytes().to_vec()]
}

#[test]
fn a_request_during_pause_is_accepted_and_applies_after_resume() -> Result<(), TestError> {
    let _guard = worker_test_lock();
    let (gate, remote, calls) = (
        Arc::new(Gate::default()),
        Arc::new(Remote::default()),
        Calls::default(),
    );
    let (_open, _unpark) = (Opened(gate.clone()), Unpark(remote.clone()));
    let probe = gated_probe(&gate, &calls);
    let (handle, collector) = spawn_scan(
        &probe,
        vec![0],
        1,
        1,
        Arc::new(RemoteContext(remote.clone())),
    );
    gate.until(|state| state.entered == 1)?;
    remote.set(|state| state.paused = true)?;
    // Accepted only acknowledges the request; nothing is assigned while paused.
    assert_eq!(
        handle.prioritize(0, d("d2"), "d2".into()),
        Ok(ScanPriorityOutcome::Accepted {
            remaining_directories: 1
        })
    );
    gate.release();
    remote.until(|state| state.active == 1 && state.parked == 1)?;
    let mark = calls.lock().map_err(|_| "record lock")?.len();
    thread::sleep(Duration::from_millis(100));
    assert_eq!(calls.lock().map_err(|_| "record lock")?.len(), mark);
    remote.set(|state| state.paused = false)?;
    let run = collector.join().map_err(|_| "collector panicked")??;
    assert_eq!(run.completeness, ResultCompleteness::Complete);
    assert_eq!(run.rows.len(), SHAPE.items());
    // Without the area, "d1" was next; after resume "d2" is assigned first.
    let calls = calls.lock().map_err(|_| "record lock")?;
    assert_eq!(calls.get(mark), Some(&(d("d2"), Call::Open)));
    assert_eq!(calls.get(mark + 1), Some(&(d("d2"), Call::Read)));
    Ok(())
}

#[test]
fn a_request_during_cancellation_assigns_no_more_work() -> Result<(), TestError> {
    let _guard = worker_test_lock();
    let (gate, remote, calls) = (
        Arc::new(Gate::default()),
        Arc::new(Remote::default()),
        Calls::default(),
    );
    let (_open, _unpark) = (Opened(gate.clone()), Unpark(remote.clone()));
    let probe = gated_probe(&gate, &calls);
    let (handle, collector) = spawn_scan(
        &probe,
        vec![0],
        1,
        1,
        Arc::new(RemoteContext(remote.clone())),
    );
    gate.until(|state| state.entered == 1)?;
    remote.set(|state| state.cancelled = true)?;
    // The scheduler is still attached, so the request is acknowledged, but
    // the cancelled scan assigns no directory after the open in flight.
    assert_eq!(
        handle.prioritize(0, d("d2"), "d2".into()),
        Ok(ScanPriorityOutcome::Accepted {
            remaining_directories: 1
        })
    );
    let recorded = calls.lock().map_err(|_| "record lock")?.len();
    gate.release();
    let run = collector.join().map_err(|_| "collector panicked")??;
    assert!(!run.outcome.completed_scope);
    assert_eq!(run.completeness, ResultCompleteness::Cancelled);
    assert_eq!(calls.lock().map_err(|_| "record lock")?.len(), recorded);
    assert_eq!(probe.live.load(Ordering::SeqCst), 0);
    assert_eq!(
        handle.prioritize(0, d("d2"), "d2".into()),
        Err(ScanControlError::AlreadyTerminal)
    );
    Ok(())
}

const RACE_TEST: &str = "requests_racing_the_end_of_the_job_neither_panic_nor_block_nor_leak";
const RACE_ROUNDS: u32 = 40;
/// Time for every round together. A round normally ends in milliseconds; one
/// shared limit lets a round slowed by a loaded runner use the time the other
/// rounds did not need, so only a job or racer that really blocks exhausts it.
const RACE_LIMIT: Duration = Duration::from_secs(120);

/// Names the test, the round and the time since the first round started.
fn race_point(round: u32, started: Instant) -> String {
    format!(
        "{RACE_TEST}: round {round} after {:?} (limit {RACE_LIMIT:?} for {RACE_ROUNDS} rounds)",
        started.elapsed()
    )
}

/// Requests one racer made before it observed the end of the job.
#[derive(Clone, Copy, Default)]
struct Raced {
    /// Answered by the attached scheduler.
    live: usize,
    /// Made before the scan body attached its scheduler.
    early: usize,
}

/// Sends requests until the job reports its end, yielding after each one so
/// the racer shares its core with the workers it races.
fn race(
    handle: &ScanControlHandle,
    round: u32,
    racer: u32,
    started: Instant,
) -> Result<Raced, String> {
    let mut raced = Raced::default();
    loop {
        if started.elapsed() >= RACE_LIMIT {
            let point = race_point(round, started);
            return Err(format!("{point}: racer {racer} never observed the end"));
        }
        let pick = (round + racer + (raced.live + raced.early) as u32) % 4;
        let path = if pick == 3 {
            Vec::new()
        } else {
            d(&format!("d{pick}"))
        };
        match handle.prioritize(pick % 2, path, format!("race-{pick}")) {
            Ok(_) => raced.live += 1,
            Err(ScanControlError::NotActive) => raced.early += 1,
            Err(ScanControlError::AlreadyTerminal) => return Ok(raced),
            Err(error) => {
                let point = race_point(round, started);
                return Err(format!("{point}: racer {racer} got {error:?}"));
            }
        }
        thread::yield_now();
    }
}

/// Joins `thread` once it finishes, failing when the shared limit runs out.
fn join_in_time<T>(
    thread: thread::JoinHandle<T>,
    what: &str,
    round: u32,
    started: Instant,
) -> Result<T, String> {
    while !thread.is_finished() {
        if started.elapsed() >= RACE_LIMIT {
            return Err(format!(
                "{}: {what} did not finish",
                race_point(round, started)
            ));
        }
        thread::sleep(Duration::from_millis(1));
    }
    let joined = thread.join();
    joined.map_err(|_| format!("{}: {what} panicked", race_point(round, started)))
}

#[test]
fn requests_racing_the_end_of_the_job_neither_panic_nor_block_nor_leak() -> Result<(), TestError> {
    let _guard = worker_test_lock();
    let shape = Shape {
        depth: 2,
        dirs: 3,
        files: 2,
        batch: 2,
    };
    let (started, mut live_rounds, mut total) = (Instant::now(), 0, Raced::default());
    for round in 0..RACE_ROUNDS {
        let probe = Probe::quiet(shape, 2);
        let context = Arc::new(Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        });
        let (handle, collector) = spawn_scan(&probe, vec![0, 1], 4, 1, context);
        let racers: Vec<_> = (0..2_u32)
            .map(|racer| {
                let handle = handle.clone();
                thread::spawn(move || race(&handle, round, racer, started))
            })
            .collect();
        let run = join_in_time(collector, "collector", round, started)??;
        let mut live = 0;
        for racer in racers {
            let raced = join_in_time(racer, "racer", round, started)??;
            live += raced.live;
            total.live += raced.live;
            total.early += raced.early;
        }
        live_rounds += usize::from(live > 0);
        let point = || race_point(round, started);
        assert!(run.outcome.completed_scope, "{}", point());
        assert_eq!(
            run.completeness,
            ResultCompleteness::Complete,
            "{}",
            point()
        );
        assert_eq!(run.rows.len(), 2 * shape.items(), "{}", point());
        assert_eq!(probe.live.load(Ordering::SeqCst), 0, "{}", point());
        assert_eq!(
            handle.prioritize(0, Vec::new(), "terminal".into()),
            Err(ScanControlError::AlreadyTerminal),
            "{}",
            point()
        );
    }
    // Every racer saw the end. A round raced it only if the attached scheduler
    // answered a request first; a loaded runner can start both racers of a
    // round late, so the test requires at least one such round.
    assert!(
        live_rounds > 0,
        "{RACE_TEST}: no round raced the end after {:?}: {} live and {} early requests",
        started.elapsed(),
        total.live,
        total.early
    );
    Ok(())
}
