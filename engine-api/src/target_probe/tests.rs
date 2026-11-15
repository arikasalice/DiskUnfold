use super::*;
mod capacity;
mod runtime;
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

/// Probe limit for tests whose in-time step is a call that returns at once.
///
/// The probe compares `Instant::now()` with its deadline, so tests cannot
/// move time. A call that must time out is held by a gate until the test
/// releases it; a call that must finish in time returns immediately, and this
/// limit leaves seconds of margin for a loaded runner to schedule its thread.
const IN_TIME_LIMIT: Duration = Duration::from_secs(2);

impl JobClock for FixedClock {
    fn now_ms(&self) -> i64 {
        5
    }
}

struct Context {
    cancelled: AtomicBool,
    events: Arc<Mutex<Vec<TargetStatusChanged>>>,
    /// Signalled once per flow when its probe I/O thread finishes the flow.
    finished: Option<mpsc::Sender<()>>,
}
struct TestFlow(
    Arc<Mutex<Vec<TargetStatusChanged>>>,
    Option<mpsc::Sender<()>>,
);
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
    fn finish(self: Box<Self>, _: JobCounts) {
        if let Some(finished) = &self.1 {
            let _ = finished.send(());
        }
    }
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
        Ok(Box::new(TestFlow(
            self.events.clone(),
            self.finished.clone(),
        )))
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
        finished: None,
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
            .to_string(),
        "1970-01-01T00:00:00.005Z"
    );
    Ok(())
}

#[test]
fn blocked_call_times_out_without_delaying_fast_target_or_emitting_late_result()
-> Result<(), Box<dyn std::error::Error>> {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let (finished, flows_finished) = mpsc::channel();
    let context = Arc::new(Context {
        cancelled: AtomicBool::new(false),
        events: events.clone(),
        finished: Some(finished),
    });
    // "slow" stays blocked until the gate opens, so it exceeds any limit;
    // "fast" returns at once and is reported before that limit expires.
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
        timeout: IN_TIME_LIMIT,
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
    // Both I/O threads have finished their flows, so no report can follow.
    for _ in 0..2 {
        flows_finished.recv_timeout(Duration::from_secs(5))?;
    }
    assert_eq!(events.lock().map(|events| events.len()).unwrap_or(0), 2);
    Ok(())
}
#[test]
fn cancellation_reports_unknown_without_waiting_for_blocked_call()
-> Result<(), Box<dyn std::error::Error>> {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (started, wait_started) = mpsc::sync_channel(1);
    let events = Arc::new(Mutex::new(Vec::new()));
    let context = Arc::new(Context {
        cancelled: AtomicBool::new(false),
        events: events.clone(),
        finished: None,
    });
    let body = TargetProbe {
        clock: Arc::new(FixedClock),
        targets: vec![probe_target("slow")],
        volumes: Arc::new(SignalPort {
            gate: gate.clone(),
            started,
        }),
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
    // Cancel only once the call is blocked inside the port.
    wait_started.recv_timeout(Duration::from_secs(5))?;
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
