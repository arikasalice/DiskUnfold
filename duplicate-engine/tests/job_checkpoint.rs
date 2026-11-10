//! Runtime cancellation reaches the registered verifier through an injected port.
use du_core_contracts::{
    Id,
    job::{
        HistoryAppend, HistoryMode, JobBody, JobContext, JobControl, JobCounts, JobDescriptor,
        JobHistory, JobOutcome, JobState, JobSummary, ProgressUnit, ResourceClass,
    },
};
use du_duplicate_engine::{
    RegisteredHash,
    candidates::{EntryKind, FileIdentity, Placeholder, Size, Target},
    verification::{OpenFile, ReadError, ReadPort, Snapshot, State, VerificationTarget},
};
use du_job_runtime::{
    CancelResult, JobClock, JobEvent, JobEvents, JobIdentity, JobPriority, JobRuntime, Submission,
    SubmitResult,
};
use std::sync::{
    Arc, Barrier, Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Identity;
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes([1; 16])
    }
}
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        1
    }
}
struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}
struct Events;
impl JobEvents for Events {
    fn publish(&self, _: JobEvent) {}
}

struct Gate {
    entered: Barrier,
    release: Barrier,
    opens: AtomicUsize,
    reads: AtomicUsize,
}
struct Port(Arc<Gate>);
struct File {
    gate: Arc<Gate>,
    identity: FileIdentity,
}
impl ReadPort for Port {
    fn open_bound<'a>(
        &'a mut self,
        _: u32,
        identity: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
        self.0.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(File {
            gate: Arc::clone(&self.0),
            identity: identity.clone(),
        }))
    }
}
impl OpenFile for File {
    fn snapshot(&mut self) -> Result<Snapshot, ReadError> {
        Ok(Snapshot {
            identity: self.identity.clone(),
            size: 3,
            modified: 7,
            change_stamp: Some(1),
        })
    }
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, ReadError> {
        if self.gate.reads.fetch_add(1, Ordering::SeqCst) == 0 {
            self.gate.entered.wait();
            self.gate.release.wait();
        }
        let bytes = b"abc".get(offset as usize..).unwrap_or_default();
        let count = bytes.len().min(buffer.len());
        buffer[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}
fn targets() -> Vec<VerificationTarget> {
    (1..=2)
        .map(|n| {
            let identity = FileIdentity {
                volume: vec![1],
                file: vec![n],
            };
            VerificationTarget {
                target: Target {
                    item_no: u32::from(n),
                    kind: EntryKind::File,
                    size: Size::Exact(3),
                    identity: Some(identity.clone()),
                    placeholder: Placeholder::Local,
                },
                snapshot: Some(Snapshot {
                    identity,
                    size: 3,
                    modified: 7,
                    change_stamp: Some(1),
                }),
            }
        })
        .collect()
}
type Evidence = (bool, Vec<u32>, Vec<u32>, u64, State);

struct Body {
    gate: Arc<Gate>,
    evidence: Mutex<Option<Evidence>>,
}
impl JobBody for Body {
    fn kind_id(&self) -> &str {
        "duplicate.check"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let result = RegisteredHash::resolve("blake3", Some(1)).and_then(|hash| {
            hash.verify_with_context(
                &targets(),
                &mut Port(Arc::clone(&self.gate)),
                context.as_ref(),
            )
        });
        match result {
            Ok(result) => {
                let value = result.evidence();
                if let Ok(mut evidence) = self.evidence.lock() {
                    *evidence = Some((
                        value.cancelled,
                        value.unjudged.clone(),
                        value.interrupted.clone(),
                        value.full_reads.observed,
                        value.states[&1],
                    ));
                }
                JobOutcome {
                    counts: JobCounts::default(),
                    completed_scope: !value.cancelled,
                    failure_reason_key: None,
                    unprocessed_summary: None,
                    result_version_id: None,
                }
            }
            Err(_) => JobOutcome {
                counts: JobCounts::default(),
                completed_scope: false,
                failure_reason_key: Some("test.verify_failed".into()),
                unprocessed_summary: None,
                result_version_id: None,
            },
        }
    }
}

#[test]
fn runtime_cancellation_waits_for_inflight_file_and_stops_new_opens()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Identity),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events),
    )?);
    // Test descriptor exercises cancellation only; content-I/O scheduling and pause are not implemented.
    runtime.register(JobDescriptor {
        kind_id: "duplicate.check".into(),
        version: 1,
        display_name_key: "duplicate.check".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["file".into()],
        progress_unit: ProgressUnit::Bytes,
        resource_classes: vec![ResourceClass::ContentIo],
        history: HistoryMode::Summary,
    })?;
    let gate = Arc::new(Gate {
        entered: Barrier::new(2),
        release: Barrier::new(2),
        opens: AtomicUsize::new(0),
        reads: AtomicUsize::new(0),
    });
    let body = Arc::new(Body {
        gate: Arc::clone(&gate),
        evidence: Mutex::new(None),
    });
    let job = match runtime.submit(Submission {
        kind_id: "duplicate.check".into(),
        deduplication_key: None,
        body: body.clone(),
        priority: JobPriority::Normal,
        scope_summary: "test".into(),
        correlation_id: Id::from_v7_bytes([2; 16]),
        sample: false,
    })? {
        SubmitResult::Created(id) => id,
        _ => return Err("expected created job".into()),
    };
    let worker_runtime = Arc::clone(&runtime);
    let worker = std::thread::spawn(move || worker_runtime.run_next());
    gate.entered.wait();
    assert_eq!(
        runtime.cancel(&job)?,
        CancelResult::Accepted(JobState::CancelRequested)
    );
    assert_eq!(runtime.state(&job)?, JobState::CancelRequested);
    gate.release.wait();
    worker.join().map_err(|_| "worker panicked")??;
    assert_eq!(runtime.state(&job)?, JobState::Cancelled);
    assert_eq!(gate.opens.load(Ordering::SeqCst), 1);
    assert_eq!(
        *body.evidence.lock().map_err(|_| "evidence poisoned")?,
        Some((true, vec![1, 2], vec![1], 3, State::Candidate))
    );
    Ok(())
}

#[test]
fn context_bridge_handles_pre_cancel_and_normal_verification() -> Result<(), String> {
    use du_core_contracts::job::{Checkpoint, JobFlow, JobFlowError};
    struct Context(Checkpoint);
    impl JobContext for Context {
        fn checkpoint(&self) -> Checkpoint {
            self.0
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Err(JobFlowError::UnsupportedResource)
        }
    }
    let make_gate = || {
        Arc::new(Gate {
            entered: Barrier::new(2),
            release: Barrier::new(2),
            opens: AtomicUsize::new(0),
            reads: AtomicUsize::new(0),
        })
    };
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?;
    let gate = make_gate();
    let cancelled = hash
        .verify_with_context(
            &targets(),
            &mut Port(Arc::clone(&gate)),
            &Context(Checkpoint::Cancel),
        )
        .map_err(|e| format!("{e:?}"))?;
    assert!(cancelled.evidence().cancelled);
    assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
    let gate = make_gate();
    let release = Arc::clone(&gate);
    let worker = std::thread::spawn(move || {
        release.entered.wait();
        release.release.wait();
    });
    let result = hash
        .verify_with_context(
            &targets(),
            &mut Port(Arc::clone(&gate)),
            &Context(Checkpoint::Continue),
        )
        .map_err(|e| format!("{e:?}"))?;
    worker.join().map_err(|_| "worker panicked")?;
    assert!(!result.evidence().cancelled);
    assert_eq!(result.evidence().groups.len(), 1);
    assert_eq!(gate.opens.load(Ordering::SeqCst), 2);
    Ok(())
}
