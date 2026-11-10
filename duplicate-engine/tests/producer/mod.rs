use super::*;
use du_core_contracts::{ItemRef, result::ResultCompleteness};
use du_duplicate_engine::{
    job::{self, Input, PreparedJob},
    result_store::{ResultStore, ScopeEvidence},
};
use du_job_runtime::FlowLimits;
use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

fn source() -> Id {
    Id::from_v7_bytes([3; 16])
}
fn result_id() -> Id {
    Id::from_v7_bytes([4; 16])
}
fn scope() -> ScopeEvidence {
    ScopeEvidence {
        roots: vec![ItemRef {
            result_version: source(),
            item_no: 0,
        }],
        exclusions: vec![],
        minimum_bytes: 1,
        outside: 2,
        excluded: 3,
        below_minimum: 4,
        unavailable: 5,
        source_completeness: ResultCompleteness::Complete,
        observed_at: None,
    }
}
fn gate() -> Arc<Gate> {
    Arc::new(Gate {
        entered: Barrier::new(2),
        release: Barrier::new(2),
        opens: AtomicUsize::new(0),
        reads: AtomicUsize::new(0),
        drops: AtomicUsize::new(0),
    })
}
fn store() -> Result<Arc<ResultStore>, String> {
    let store = Arc::new(ResultStore::new(
        NonZeroUsize::MIN.saturating_add(3),
        NonZeroUsize::MIN.saturating_add(15),
    ));
    store
        .register_source(source())
        .map_err(|e| format!("{e:?}"))?;
    Ok(store)
}
fn setup() -> Result<Arc<JobRuntime>, String> {
    let runtime = JobRuntime::new_with_content_io_limit(
        Arc::new(Identity),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events),
        FlowLimits::disabled(),
        1,
    )
    .map_err(|e| format!("{e:?}"))?;
    runtime
        .register(job::descriptor())
        .map_err(|e| format!("{e:?}"))?;
    Ok(Arc::new(runtime))
}
fn prepare(gate: Arc<Gate>, store: Arc<ResultStore>) -> Result<PreparedJob, String> {
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    job::prepare(
        Input {
            source: source(),
            result: result_id(),
            scope: scope(),
            targets: targets(),
            reader: Port(gate),
        },
        hash,
        store,
    )
    .map_err(|e| format!("{e:?}"))
}
fn submit(runtime: &JobRuntime, prepared: PreparedJob) -> Result<Id, String> {
    let submission = Submission {
        kind_id: "duplicate.check".into(),
        deduplication_key: None,
        body: prepared.body,
        priority: JobPriority::Normal,
        scope_summary: "test scope".into(),
        correlation_id: Id::from_v7_bytes([2; 16]),
        sample: false,
    };
    match runtime
        .submit_duplicate_terminal(submission, prepared.queued, prepared.commit)
        .map_err(|e| format!("{e:?}"))?
    {
        SubmitResult::Created(id) => Ok(id),
        _ => Err("expected created job".into()),
    }
}
fn wait_state(runtime: &JobRuntime, id: &Id, wanted: JobState) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let state = runtime.state(id).map_err(|e| format!("{e:?}"))?;
        if state == wanted {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(format!("expected {wanted:?}, got {state:?}"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn queued_cancel_publishes_unread_scope_and_candidate() -> Result<(), String> {
    let runtime = setup()?;
    let store = store()?;
    let gate = gate();
    let id = submit(&runtime, prepare(Arc::clone(&gate), Arc::clone(&store))?)?;
    assert!(matches!(
        runtime.cancel(&id),
        Ok(CancelResult::Accepted(JobState::Cancelled))
    ));
    let pinned = store
        .pin(Some(&result_id()))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(pinned.scope(), Some(&scope()));
    assert!(pinned.outcome().evidence().cancelled);
    assert_eq!(
        pinned.content_coverage(),
        du_duplicate_engine::result_store::ContentCoverage::Cancelled
    );
    assert_eq!(pinned.outcome().evidence().unjudged, vec![1, 2]);
    assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
    assert_eq!(
        runtime
            .snapshot(&id)
            .map_err(|e| format!("{e:?}"))?
            .result_version_id,
        Some(result_id())
    );
    Ok(())
}

#[test]
fn running_cancel_waits_for_read_and_publishes_partial() -> Result<(), String> {
    let runtime = setup()?;
    let store = store()?;
    let gate = gate();
    let id = submit(&runtime, prepare(Arc::clone(&gate), Arc::clone(&store))?)?;
    let worker_runtime = Arc::clone(&runtime);
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    gate.entered.wait();
    assert!(matches!(
        runtime.cancel(&id),
        Ok(CancelResult::Accepted(JobState::CancelRequested))
    ));
    assert!(store.pin(Some(&result_id())).is_err());
    gate.release.wait();
    worker
        .join()
        .map_err(|_| "worker panicked")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        runtime.state(&id).map_err(|e| format!("{e:?}"))?,
        JobState::Cancelled
    );
    let pinned = store
        .pin(Some(&result_id()))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(pinned.scope(), Some(&scope()));
    let outcome = pinned.outcome().evidence();
    assert!(outcome.cancelled);
    assert_eq!(outcome.interrupted, vec![1]);
    assert_eq!(outcome.unjudged, vec![1, 2]);
    assert_eq!(outcome.full_reads.observed, 3);
    assert_eq!(gate.opens.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn pause_happens_after_file_handle_is_closed_and_resume_finishes() -> Result<(), String> {
    let runtime = setup()?;
    let store = store()?;
    let gate = gate();
    let id = submit(&runtime, prepare(Arc::clone(&gate), Arc::clone(&store))?)?;
    let worker_runtime = Arc::clone(&runtime);
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    gate.entered.wait();
    assert!(matches!(
        runtime.pause(&id),
        Ok((CancelResult::Accepted(JobState::PauseRequested), _))
    ));
    gate.release.wait();
    wait_state(&runtime, &id, JobState::Paused)?;
    assert_eq!(gate.opens.load(Ordering::SeqCst), 1);
    assert_eq!(gate.drops.load(Ordering::SeqCst), 1);
    assert!(store.pin(Some(&result_id())).is_err());
    assert!(matches!(
        runtime.resume(&id),
        Ok((CancelResult::Accepted(JobState::Running), _))
    ));
    worker
        .join()
        .map_err(|_| "worker panicked")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        runtime.state(&id).map_err(|e| format!("{e:?}"))?,
        JobState::Done
    );
    let result = store
        .pin(Some(&result_id()))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.outcome().evidence().groups.len(), 1);
    assert_eq!(
        result.content_coverage(),
        du_duplicate_engine::result_store::ContentCoverage::Partial
    );
    assert_eq!(gate.opens.load(Ordering::SeqCst), 2);
    Ok(())
}

struct Sequence(AtomicUsize);
impl JobIdentity for Sequence {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes([self.0.fetch_add(1, Ordering::SeqCst) as u8; 16])
    }
}

#[test]
fn saturated_content_slot_waits_then_releases_for_next_job() -> Result<(), String> {
    let runtime = Arc::new(
        JobRuntime::new_with_content_io_limit(
            Arc::new(Sequence(AtomicUsize::new(10))),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events),
            FlowLimits::disabled(),
            1,
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    runtime
        .register(job::descriptor())
        .map_err(|e| format!("{e:?}"))?;
    let store = store()?;
    let first_gate = gate();
    let second_gate = gate();
    let first = submit(
        &runtime,
        prepare(Arc::clone(&first_gate), Arc::clone(&store))?,
    )?;
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let second_prepared = job::prepare(
        Input {
            source: source(),
            result: Id::from_v7_bytes([5; 16]),
            scope: scope(),
            targets: targets(),
            reader: Port(Arc::clone(&second_gate)),
        },
        hash,
        Arc::clone(&store),
    )
    .map_err(|e| format!("{e:?}"))?;
    let second = submit(&runtime, second_prepared)?;
    let worker_runtime = Arc::clone(&runtime);
    let worker1 = std::thread::spawn(move || worker_runtime.run_next());
    first_gate.entered.wait();
    let worker_runtime = Arc::clone(&runtime);
    let worker2 = std::thread::spawn(move || worker_runtime.run_next());
    wait_state(&runtime, &second, JobState::Running)?;
    assert_eq!(second_gate.opens.load(Ordering::SeqCst), 0);
    first_gate.release.wait();
    worker1
        .join()
        .map_err(|_| "worker1 panicked")?
        .map_err(|e| format!("{e:?}"))?;
    second_gate.entered.wait();
    second_gate.release.wait();
    worker2
        .join()
        .map_err(|_| "worker2 panicked")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        runtime.state(&first).map_err(|e| format!("{e:?}"))?,
        JobState::Done
    );
    assert_eq!(
        runtime.state(&second).map_err(|e| format!("{e:?}"))?,
        JobState::Done
    );
    assert_eq!(first_gate.drops.load(Ordering::SeqCst), 2);
    assert_eq!(second_gate.drops.load(Ordering::SeqCst), 2);
    assert!(store.pin(Some(&Id::from_v7_bytes([5; 16]))).is_ok());
    Ok(())
}

#[test]
fn cancellation_while_waiting_for_content_slot_publishes_unread_partial() -> Result<(), String> {
    let runtime = Arc::new(
        JobRuntime::new_with_content_io_limit(
            Arc::new(Sequence(AtomicUsize::new(20))),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events),
            FlowLimits::disabled(),
            1,
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    runtime
        .register(job::descriptor())
        .map_err(|e| format!("{e:?}"))?;
    let store = store()?;
    let first_gate = gate();
    let second_gate = gate();
    let first = submit(
        &runtime,
        prepare(Arc::clone(&first_gate), Arc::clone(&store))?,
    )?;
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let second_result = Id::from_v7_bytes([6; 16]);
    let prepared = job::prepare(
        Input {
            source: source(),
            result: second_result.clone(),
            scope: scope(),
            targets: targets(),
            reader: Port(Arc::clone(&second_gate)),
        },
        hash,
        Arc::clone(&store),
    )
    .map_err(|e| format!("{e:?}"))?;
    let second = submit(&runtime, prepared)?;
    let worker_runtime = Arc::clone(&runtime);
    let worker1 = std::thread::spawn(move || worker_runtime.run_next());
    first_gate.entered.wait();
    let worker_runtime = Arc::clone(&runtime);
    let worker2 = std::thread::spawn(move || worker_runtime.run_next());
    wait_state(&runtime, &second, JobState::Running)?;
    assert!(matches!(
        runtime.cancel(&second),
        Ok(CancelResult::Accepted(JobState::CancelRequested))
    ));
    worker2
        .join()
        .map_err(|_| "worker2 panicked")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(second_gate.opens.load(Ordering::SeqCst), 0);
    let pinned = store
        .pin(Some(&second_result))
        .map_err(|e| format!("{e:?}"))?;
    assert!(pinned.outcome().evidence().cancelled);
    assert_eq!(pinned.outcome().evidence().unjudged, vec![1, 2]);
    first_gate.release.wait();
    worker1
        .join()
        .map_err(|_| "worker1 panicked")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        runtime.state(&first).map_err(|e| format!("{e:?}"))?,
        JobState::Done
    );
    assert_eq!(
        runtime.state(&second).map_err(|e| format!("{e:?}"))?,
        JobState::Cancelled
    );
    Ok(())
}

#[test]
fn publication_capacity_failure_keeps_prior_result_and_reports_failure() -> Result<(), String> {
    let runtime = Arc::new(
        JobRuntime::new_with_content_io_limit(
            Arc::new(Sequence(AtomicUsize::new(30))),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events),
            FlowLimits::disabled(),
            1,
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    runtime
        .register(job::descriptor())
        .map_err(|e| format!("{e:?}"))?;
    let store = Arc::new(ResultStore::new(
        NonZeroUsize::MIN,
        NonZeroUsize::MIN.saturating_add(10),
    ));
    store
        .register_source(source())
        .map_err(|e| format!("{e:?}"))?;
    let first_gate = gate();
    let first = submit(
        &runtime,
        prepare(Arc::clone(&first_gate), Arc::clone(&store))?,
    )?;
    let worker_runtime = Arc::clone(&runtime);
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    first_gate.entered.wait();
    first_gate.release.wait();
    worker
        .join()
        .map_err(|_| "first worker panicked")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        runtime.state(&first).map_err(|e| format!("{e:?}"))?,
        JobState::Done
    );
    let second_gate = gate();
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let second_result = Id::from_v7_bytes([7; 16]);
    let prepared = job::prepare(
        Input {
            source: source(),
            result: second_result.clone(),
            scope: scope(),
            targets: targets(),
            reader: Port(Arc::clone(&second_gate)),
        },
        hash,
        Arc::clone(&store),
    )
    .map_err(|e| format!("{e:?}"))?;
    let second = submit(&runtime, prepared)?;
    let worker_runtime = Arc::clone(&runtime);
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    second_gate.entered.wait();
    second_gate.release.wait();
    worker
        .join()
        .map_err(|_| "second worker panicked")?
        .map_err(|e| format!("{e:?}"))?;
    let snapshot = runtime.snapshot(&second).map_err(|e| format!("{e:?}"))?;
    assert_eq!(snapshot.state, JobState::Partial);
    assert_eq!(
        snapshot.failure_reason_key.as_deref(),
        Some("support.internal_error")
    );
    assert_eq!(snapshot.result_version_id, None);
    assert!(store.pin(Some(&second_result)).is_err());
    assert_eq!(
        store.pin(None).map_err(|e| format!("{e:?}"))?.version(),
        &result_id()
    );
    Ok(())
}

mod boundary;
