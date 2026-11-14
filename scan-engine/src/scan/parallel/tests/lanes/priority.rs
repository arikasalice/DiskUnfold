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
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
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
        handle.prioritize(0, d("d2")),
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
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
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
        handle.prioritize(0, d("d2")),
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
        handle.prioritize(0, d("d2")),
        Err(ScanControlError::AlreadyTerminal)
    );
    Ok(())
}

#[test]
fn requests_racing_the_end_of_the_job_neither_panic_nor_block_nor_leak() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let shape = Shape {
        depth: 2,
        dirs: 3,
        files: 2,
        batch: 2,
    };
    for round in 0..40_u32 {
        let probe = Probe::quiet(shape, 2);
        let context = Arc::new(Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        });
        let (handle, collector) = spawn_scan(&probe, vec![0, 1], 4, 1, context);
        let racers: Vec<_> = (0..2_u32)
            .map(|racer| {
                let handle = handle.clone();
                thread::spawn(move || -> Result<usize, ScanControlError> {
                    let deadline = Instant::now() + Duration::from_secs(20);
                    let mut requests = 0;
                    while Instant::now() < deadline {
                        let pick = (round + racer + requests as u32) % 4;
                        let path = if pick == 3 {
                            Vec::new()
                        } else {
                            d(&format!("d{pick}"))
                        };
                        match handle.prioritize(pick % 2, path) {
                            Ok(_) | Err(ScanControlError::NotActive) => requests += 1,
                            Err(ScanControlError::AlreadyTerminal) => return Ok(requests),
                            Err(error) => return Err(error),
                        }
                    }
                    Err(ScanControlError::Unavailable)
                })
            })
            .collect();
        let run = collector.join().map_err(|_| "collector panicked")??;
        for racer in racers {
            racer
                .join()
                .map_err(|_| "racer panicked")?
                .map_err(|error| format!("{error:?}"))?;
        }
        assert!(run.outcome.completed_scope, "round {round}");
        assert_eq!(run.completeness, ResultCompleteness::Complete);
        assert_eq!(run.rows.len(), 2 * shape.items(), "round {round}");
        assert_eq!(probe.live.load(Ordering::SeqCst), 0, "round {round}");
        assert_eq!(
            handle.prioritize(0, Vec::new()),
            Err(ScanControlError::AlreadyTerminal)
        );
    }
    Ok(())
}
