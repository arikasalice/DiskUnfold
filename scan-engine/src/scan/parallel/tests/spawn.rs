//! Verify cleanup when only part of a worker group can start.
use super::*;

#[test]
fn failed_second_spawn_closes_channels_and_finishes_all_flows()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let mut previous = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
    previous
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let baseline = previous
        .seal(ids.next_id(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    TEST_WORKER_STARTED.store(false, Ordering::Release);
    FAIL_SPAWN_AT.store(1, Ordering::Release);
    let reads = Arc::new(AtomicUsize::new(0));
    let result = collect_parallel(
        &sink,
        ids.next_id(),
        Box::new(Root {
            complete: false,
            reads: reads.clone(),
        }),
        &ids,
        &context,
        3,
        &|_| true,
    );
    FAIL_SPAWN_AT.store(usize::MAX, Ordering::Release);
    assert!(TEST_WORKER_STARTED.load(Ordering::Acquire));
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    assert!(result.result_version_id.is_none());
    assert_eq!(context.completed.load(Ordering::SeqCst), 4);
    assert_eq!(context.abandoned.load(Ordering::SeqCst), 0);
    assert_eq!(
        sink.pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing baseline")?
            .version_id(),
        baseline.version_id()
    );
    Ok(())
}
