//! Shallow directories are assigned first and a marked area precedes them.
use super::*;
use std::collections::HashSet;

type Calls = Arc<Mutex<Vec<(Vec<Vec<u8>>, Call)>>>;

fn context() -> Context {
    Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    }
}

#[test]
fn one_worker_reads_every_directory_of_a_depth_before_any_deeper_one() -> Result<(), TestError> {
    let _guard = worker_test_lock();
    let shape = Shape {
        depth: 3,
        dirs: 3,
        files: 2,
        batch: 2,
    };
    let first_reads = Arc::new(Mutex::new((HashSet::new(), Vec::new())));
    let record = first_reads.clone();
    let probe = Probe::new(
        shape,
        1,
        Box::new(move |_, path, call| {
            let mut seen = record.lock().map_err(|_| io_error("test.record"))?;
            if call == Call::Read && seen.0.insert(path.to_vec()) {
                seen.1.push(path.len());
            }
            Ok(())
        }),
    );
    let run = run_forest(&probe, vec![0], 1, 4, &context())?;
    assert_eq!(run.completeness, ResultCompleteness::Complete);
    let depths = first_reads.lock().map_err(|_| "record lock")?.1.clone();
    assert_eq!(depths.len(), shape.directories());
    // The tree never fills the lane queue, so no overflow stack reorders it.
    assert!(
        depths.windows(2).all(|pair| pair[0] <= pair[1]),
        "{depths:?}"
    );
    Ok(())
}

/// Index of the first call at or after `from` that is not below `area`.
fn first_outside(calls: &[(Vec<Vec<u8>>, Call)], from: usize, area: &[u8]) -> usize {
    calls[from..]
        .iter()
        .position(|(path, _)| path.first().map(Vec::as_slice) != Some(area))
        .map_or(calls.len(), |offset| from + offset)
}

#[test]
fn a_marked_area_is_assigned_before_shallower_normal_work() -> Result<(), TestError> {
    let _guard = worker_test_lock();
    let shape = Shape {
        depth: 2,
        dirs: 3,
        files: 0,
        batch: 8,
    };
    let gate = Arc::new(Gate::default());
    let _open = Opened(gate.clone());
    let calls: Calls = Arc::default();
    let (hook_gate, record) = (gate.clone(), calls.clone());
    // The first read of "d2" waits; by then "d0" and "d1" are opened and read.
    let probe = Probe::new(
        shape,
        1,
        Box::new(move |_, path, call| {
            record
                .lock()
                .map_err(|_| io_error("test.record"))?
                .push((path.to_vec(), call));
            if call == Call::Read && path == [b"d2".to_vec()] {
                hook_gate.wait()?;
            }
            Ok(())
        }),
    );
    let session = Arc::new(ScanControlSession::new(1));
    let handle = session.handle();
    let job_probe = probe.clone();
    let collector = thread::spawn(move || -> Result<JobOutcome, TestError> {
        let ids = Ids(AtomicUsize::new(1));
        let sink = IngestSink::default();
        let build = sink.open_forest(ids.next_id(), vec![observed_root("L:\\0", 1)]);
        let devices = DevicePlan {
            groups: vec![0],
            limit: 1,
        };
        let options = (devices, None, Some(&*session));
        Ok(collect_open_forest_diagnostic(
            build,
            job_probe.roots(),
            &ids,
            &context(),
            1,
            None,
            options,
        ))
    });
    gate.until(|state| state.entered == 1)?;
    // "d1" is queued at depth one with returned entries and no child yet.
    assert_eq!(
        handle.prioritize(0, vec![b"d1".to_vec()]),
        Ok(ScanPriorityOutcome::Accepted {
            remaining_directories: 1
        })
    );
    let mark = calls.lock().map_err(|_| "record lock")?.len();
    gate.release();
    let outcome = collector.join().map_err(|_| "collector panicked")??;
    assert!(outcome.completed_scope);
    let calls = calls.lock().map_err(|_| "record lock")?.clone();
    // After the in-flight read returns, the whole "d1" area runs first, its
    // depth-two children included, before the normal depth-one "d0" and "d2".
    let area_end = first_outside(&calls, mark, b"d1");
    let area: Vec<_> = calls[mark..area_end].iter().collect();
    assert_eq!(
        area.iter().filter(|(_, call)| *call == Call::Open).count(),
        3
    );
    assert!(area.iter().any(|(path, _)| path.len() == 2));
    assert!(
        calls[area_end..]
            .iter()
            .all(|(path, _)| path.first() != Some(&b"d1".to_vec()))
    );
    assert_eq!(
        handle.prioritize(0, vec![b"d1".to_vec()]),
        Err(ScanControlError::AlreadyTerminal)
    );
    Ok(())
}
