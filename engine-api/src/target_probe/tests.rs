use super::*;
mod capacity;
use du_core_contracts::{
    PathBytes, PathEncoding,
    analysis_ipc::{TargetAccess, TargetStatusChanged},
    job::JobFlowError,
    ports::PortError,
    volume::{CapabilityState, VolumeCapacity, VolumeEnumeration},
};
use std::sync::{
    Condvar,
    atomic::{AtomicBool, Ordering},
};

struct FixedClock;

impl JobClock for FixedClock {
    fn now_ms(&self) -> i64 {
        5
    }
}

struct Context {
    cancelled: AtomicBool,
    events: Arc<Mutex<Vec<TargetStatusChanged>>>,
}
struct TestFlow(Arc<Mutex<Vec<TargetStatusChanged>>>);
impl JobFlow for TestFlow {
    fn report_item(&self, payload: &[u8]) -> Result<(), JobFlowError> {
        let item =
            serde_json::from_slice(payload).map_err(|_| JobFlowError::DeliveryUnavailable)?;
        self.0
            .lock()
            .map_err(|_| JobFlowError::DeliveryUnavailable)?
            .push(item);
        Ok(())
    }
    fn finish(self: Box<Self>, _: JobCounts) {}
}
impl JobContext for Context {
    fn checkpoint(&self) -> Checkpoint {
        if self.cancelled.load(Ordering::SeqCst) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(TestFlow(self.events.clone())))
    }
}
struct Port(Arc<(Mutex<bool>, Condvar)>);
impl VolumePort for Port {
    fn enumerate(&self) -> VolumeEnumeration {
        unreachable!()
    }
    fn path_access(&self, path: &[u8]) -> Result<(), PortError> {
        if path == b"delay" {
            std::thread::sleep(Duration::from_millis(30));
        }
        if path == b"slow" {
            let (lock, cv) = &*self.0;
            let mut released = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while !*released {
                released = cv
                    .wait(released)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        }
        Ok(())
    }
    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        Ok(VolumeCapacity {
            total_bytes: 100,
            available_bytes: 50,
        })
    }
}
fn path(name: &str) -> OsPath {
    OsPath {
        encoding: PathEncoding::Unix,
        raw: PathBytes::from_bytes(name.as_bytes()),
        display: name.into(),
        lossy: false,
    }
}
fn probe_target(name: &str) -> ProbeTarget {
    ProbeTarget {
        path: path(name),
        volume_id: None,
    }
}
fn release(gate: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, cv) = &**gate;
    if let Ok(mut value) = lock.lock() {
        *value = true;
        cv.notify_all();
    }
}

#[test]
fn a_late_but_returned_call_is_not_counted_as_still_blocking_a_permit()
-> Result<(), Box<dyn std::error::Error>> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let context = Context {
        cancelled: AtomicBool::new(false),
        events: events.clone(),
    };
    let deadline = Instant::now();
    let (send, receiver) = mpsc::sync_channel(1);
    let (ack, acknowledged) = mpsc::sync_channel(1);
    send.send((
        deadline + Duration::from_millis(1),
        unavailable("os.query_failed", AccessStatus::Unknown),
    ))?;
    let mut pending = vec![Pending {
        path: path("late"),
        flow: Arc::new(Mutex::new(Some(
            context.try_start_flow().map_err(|e| format!("{e:?}"))?,
        ))),
        io_finished: Arc::new(AtomicBool::new(true)),
        deadline,
        receiver,
        ack,
    }];
    let mut timed_out = Vec::new();
    assert!(!process_pending(
        &context,
        &mut pending,
        &mut timed_out,
        &FixedClock
    ));
    assert!(pending.is_empty());
    assert!(timed_out.is_empty());
    assert!(acknowledged.try_recv().is_ok());
    assert_eq!(
        events.lock().map_err(|_| "probe events poisoned")?[0].access,
        TargetAccess::Unresponsive
    );
    assert_eq!(
        events.lock().map_err(|_| "probe events poisoned")?[0]
            .observed_at
            .as_str(),
        "1970-01-01T00:00:00.005Z"
    );
    Ok(())
}

#[test]
fn blocked_call_times_out_without_delaying_fast_target_or_emitting_late_result() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let context = Arc::new(Context {
        cancelled: AtomicBool::new(false),
        events: events.clone(),
    });
    let body = TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![probe_target("slow"), probe_target("fast")],
        volumes: Arc::new(Port(gate.clone())),
        capacity: Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        },
        path_access: Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        },
        timeout: Duration::from_millis(100),
    };
    let outcome = body.run(context);
    assert!(outcome.completed_scope);
    let snapshot = events
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default();
    assert_eq!(snapshot.len(), 2);
    assert_eq!(snapshot[0].target.display, "fast");
    assert_eq!(snapshot[0].access, TargetAccess::Accessible);
    assert_eq!(snapshot[1].access, TargetAccess::Unresponsive);
    release(&gate);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(events.lock().map(|events| events.len()).unwrap_or(0), 2);
}
#[test]
fn cancellation_reports_unknown_without_waiting_for_blocked_call() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let context = Arc::new(Context {
        cancelled: AtomicBool::new(false),
        events: events.clone(),
    });
    let body = TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![probe_target("slow")],
        volumes: Arc::new(Port(gate.clone())),
        capacity: Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        },
        path_access: Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        },
        timeout: Duration::from_secs(5),
    };
    let worker_context = context.clone();
    let worker = std::thread::spawn(move || body.run(worker_context));
    std::thread::sleep(Duration::from_millis(30));
    context.cancelled.store(true, Ordering::SeqCst);
    let outcome = worker.join().ok();
    assert!(outcome.is_some());
    assert_eq!(
        events
            .lock()
            .ok()
            .and_then(|events| events.first().map(|event| event.access)),
        Some(TargetAccess::Unknown)
    );
    release(&gate);
}

struct RuntimeFixture(mpsc::Sender<du_job_runtime::JobEvent>);
impl du_job_runtime::JobIdentity for RuntimeFixture {
    fn next_id(&self) -> du_core_contracts::Id {
        du_core_contracts::Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes())
    }
}
impl du_job_runtime::JobClock for RuntimeFixture {
    fn now_ms(&self) -> i64 {
        1
    }
}
impl du_job_runtime::JobEvents for RuntimeFixture {
    fn publish(&self, event: du_job_runtime::JobEvent) {
        let _ = self.0.send(event);
    }
}
impl du_core_contracts::job::JobHistory for RuntimeFixture {
    fn append(
        &self,
        _: &du_core_contracts::job::JobSummary,
    ) -> Result<du_core_contracts::job::HistoryAppend, String> {
        Err("unexpected history".into())
    }
}
#[test]
fn actual_runtime_reuses_single_permit_for_three_targets() -> Result<(), Box<dyn std::error::Error>>
{
    use du_job_runtime::{FlowLimits, JobEvent, JobIdentity, JobPriority, JobRuntime, Submission};
    let (send, recv) = mpsc::channel();
    let fixture = Arc::new(RuntimeFixture(send));
    let runtime = Arc::new(JobRuntime::new_with_limits(
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        FlowLimits {
            metadata_io: 1,
            queued_items: 16,
            item_bytes: 65536,
        },
    )?);
    runtime.register(target_probe::descriptor())?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let cap = Capability {
        state: CapabilityState::Supported,
        reason_key: None,
    };
    let body = TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![
            probe_target("delay"),
            probe_target("fast"),
            probe_target("third"),
        ],
        volumes: Arc::new(Port(gate)),
        capacity: cap.clone(),
        path_access: cap,
        timeout: Duration::from_secs(2),
    };
    runtime.submit(Submission {
        kind_id: TARGET_PROBE_KIND.into(),
        deduplication_key: None,
        body: Arc::new(body),
        priority: JobPriority::Normal,
        scope_summary: String::new(),
        correlation_id: fixture.next_id(),
        sample: false,
    })?;
    let worker = runtime.clone();
    let thread = std::thread::spawn(move || worker.run_next());
    let mut items = Vec::new();
    let mut finished = 0;
    while finished == 0 {
        match recv.recv_timeout(Duration::from_secs(3))? {
            JobEvent::ItemResult { payload, .. } => {
                items.push(serde_json::from_slice::<TargetStatusChanged>(&payload)?)
            }
            JobEvent::Finished { .. } => finished += 1,
            _ => {}
        }
    }
    thread.join().map_err(|_| "worker panic")??;
    assert_eq!(items.len(), 3);
    assert!(
        items
            .iter()
            .all(|item| item.access == TargetAccess::Accessible)
    );
    Ok(())
}

#[test]
fn actual_runtime_timeout_retains_permit_and_suppresses_late_result()
-> Result<(), Box<dyn std::error::Error>> {
    use du_job_runtime::{FlowLimits, JobEvent, JobIdentity, JobPriority, JobRuntime, Submission};
    let (send, recv) = mpsc::channel();
    let fixture = Arc::new(RuntimeFixture(send));
    let runtime = Arc::new(JobRuntime::new_with_limits(
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        FlowLimits {
            metadata_io: 1,
            queued_items: 16,
            item_bytes: 65536,
        },
    )?);
    runtime.register(target_probe::descriptor())?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let cap = Capability {
        state: CapabilityState::Supported,
        reason_key: None,
    };
    let body = TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![probe_target("slow"), probe_target("fast")],
        volumes: Arc::new(Port(gate.clone())),
        capacity: cap.clone(),
        path_access: cap,
        timeout: Duration::from_millis(100),
    };
    runtime.submit(Submission {
        kind_id: TARGET_PROBE_KIND.into(),
        deduplication_key: None,
        body: Arc::new(body),
        priority: JobPriority::Normal,
        scope_summary: String::new(),
        correlation_id: fixture.next_id(),
        sample: false,
    })?;
    let worker = runtime.clone();
    let thread = std::thread::spawn(move || worker.run_next());
    let mut items = Vec::new();
    while items.len() < 2 {
        if let JobEvent::ItemResult { payload, .. } = recv.recv_timeout(Duration::from_secs(3))? {
            items.push(serde_json::from_slice::<TargetStatusChanged>(&payload)?);
        }
    }
    assert!(
        items
            .iter()
            .all(|item| item.access == TargetAccess::Unresponsive)
    );
    release(&gate);
    loop {
        match recv.recv_timeout(Duration::from_secs(3))? {
            JobEvent::ItemResult { .. } => return Err("late item emitted".into()),
            JobEvent::Finished { .. } => break,
            _ => {}
        }
    }
    thread.join().map_err(|_| "worker panic")??;
    assert!(!matches!(
        recv.recv_timeout(Duration::from_millis(50)),
        Ok(JobEvent::ItemResult { .. } | JobEvent::Finished { .. })
    ));
    Ok(())
}

struct SignalPort {
    gate: Arc<(Mutex<bool>, Condvar)>,
    started: mpsc::SyncSender<()>,
}
impl VolumePort for SignalPort {
    fn enumerate(&self) -> VolumeEnumeration {
        unreachable!()
    }
    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        let _ = self.started.send(());
        let (lock, cv) = &*self.gate;
        let mut release = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*release {
            release = cv
                .wait(release)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        Ok(())
    }
    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        Ok(VolumeCapacity {
            total_bytes: 1,
            available_bytes: 1,
        })
    }
}

#[test]
fn actual_runtime_foreign_permit_saturation_ends_with_reason()
-> Result<(), Box<dyn std::error::Error>> {
    use du_job_runtime::{
        FlowLimits, JobEvent, JobIdentity, JobPriority, JobRuntime, Submission, SubmitResult,
    };
    let (send, recv) = mpsc::channel();
    let fixture = Arc::new(RuntimeFixture(send));
    let runtime = Arc::new(JobRuntime::new_with_limits(
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        FlowLimits {
            metadata_io: 1,
            queued_items: 16,
            item_bytes: 65536,
        },
    )?);
    runtime.register(target_probe::descriptor())?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (started, wait_started) = mpsc::sync_channel(1);
    let cap = Capability {
        state: CapabilityState::Supported,
        reason_key: None,
    };
    let submit = |body: TargetProbe| Submission {
        kind_id: TARGET_PROBE_KIND.into(),
        deduplication_key: None,
        body: Arc::new(body),
        priority: JobPriority::Normal,
        scope_summary: String::new(),
        correlation_id: fixture.next_id(),
        sample: false,
    };
    runtime.submit(submit(TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![probe_target("slow")],
        volumes: Arc::new(SignalPort {
            gate: gate.clone(),
            started,
        }),
        capacity: cap.clone(),
        path_access: cap.clone(),
        timeout: Duration::from_secs(5),
    }))?;
    let first_runtime = runtime.clone();
    let first = std::thread::spawn(move || first_runtime.run_next());
    wait_started.recv_timeout(Duration::from_secs(2))?;
    let second = match runtime.submit(submit(TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![probe_target("fast")],
        volumes: Arc::new(Port(gate.clone())),
        capacity: cap.clone(),
        path_access: cap,
        timeout: Duration::from_millis(80),
    }))? {
        SubmitResult::Created(id) => id,
        _ => return Err("unexpected existing".into()),
    };
    let second_runtime = runtime.clone();
    let second_worker = std::thread::spawn(move || second_runtime.run_next());
    loop {
        if let JobEvent::Finished { summary, .. } = recv.recv_timeout(Duration::from_secs(2))?
            && summary.job_id == second
        {
            assert_eq!(summary.failure_reason_key.as_deref(), Some("os.call_slow"));
            break;
        }
    }
    second_worker.join().map_err(|_| "second panic")??;
    release(&gate);
    first.join().map_err(|_| "first panic")??;
    Ok(())
}
