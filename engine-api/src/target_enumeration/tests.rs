use super::*;
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    volume::{Capability, CapabilityState, VolumeCapabilities, VolumeCapacity},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

#[test]
fn deadline_starts_at_admission_even_if_wait_is_delayed() -> Result<(), Box<dyn std::error::Error>>
{
    let (port, started) = fixture(Ok(vec![]));
    let worker = EnumerationWorker::new(port.clone());
    let pending = worker.start().map_err(|e| format!("{e:?}"))?;
    started.recv_timeout(Duration::from_secs(2))?;
    port.release(1);
    let mut inner = worker.shared.lock();
    while matches!(inner.state, State::Running { .. }) {
        let result = worker
            .shared
            .changed
            .wait_timeout(inner, Duration::from_secs(2))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner = result.0;
        if result.1.timed_out() {
            return Err("observation did not finish".into());
        }
    }
    drop(inner);
    assert!(matches!(pending.wait(Duration::ZERO), WaitResult::TimedOut));
    assert!(worker.shutdown(Duration::ZERO));
    Ok(())
}

struct FakePort {
    calls: AtomicUsize,
    started: mpsc::Sender<usize>,
    gate: (Mutex<usize>, Condvar),
    result: VolumeEnumeration,
}

impl FakePort {
    fn release(&self, count: usize) {
        *self
            .gate
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = count;
        self.gate.1.notify_all();
    }
}

impl VolumePort for FakePort {
    fn enumerate(&self) -> VolumeEnumeration {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.started.send(call);
        let mut permits = self
            .gate
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *permits < call {
            permits = self
                .gate
                .1
                .wait(permits)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        self.result.clone()
    }
    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        unreachable!()
    }
    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        unreachable!()
    }
}

fn fixture(
    mounts: Result<Vec<du_core_contracts::volume::VolumeMount>, PortError>,
) -> (Arc<FakePort>, mpsc::Receiver<usize>) {
    let (started, received) = mpsc::channel();
    let capability = Capability {
        state: CapabilityState::Partial,
        reason_key: Some("os.unsupported"),
    };
    (
        Arc::new(FakePort {
            calls: AtomicUsize::new(0),
            started,
            gate: (Mutex::new(0), Condvar::new()),
            result: VolumeEnumeration {
                mounts,
                capabilities: VolumeCapabilities {
                    enumerate: capability.clone(),
                    identity: capability.clone(),
                    capacity: capability.clone(),
                    path_access: capability.clone(),
                    path_compare_rule: capability.clone(),
                    mount_boundary: capability.clone(),
                    alias: capability,
                },
            },
        }),
        received,
    )
}

#[test]
fn unsupported_enumeration_is_not_changed_into_success() -> Result<(), Box<dyn std::error::Error>> {
    let (mut port, started) = fixture(Ok(vec![]));
    Arc::get_mut(&mut port)
        .ok_or("shared fixture")?
        .result
        .capabilities
        .enumerate
        .state = CapabilityState::Unsupported;
    let worker = EnumerationWorker::new(port.clone());
    let pending = worker.start().map_err(|e| format!("{e:?}"))?;
    started.recv_timeout(Duration::from_secs(2))?;
    port.release(1);
    match pending.wait(Duration::from_secs(2)) {
        WaitResult::Ready(value) => assert_eq!(
            value.capabilities.enumerate.state,
            CapabilityState::Unsupported
        ),
        _ => return Err("expected ready".into()),
    }
    Ok(())
}

#[test]
fn ready_preserves_failure_and_capabilities() -> Result<(), Box<dyn std::error::Error>> {
    let failure = PortError {
        kind: PortErrorKind::PermissionDenied,
        reason_key: "os.query_failed".into(),
        os_code: Some(5),
    };
    let (port, started) = fixture(Err(failure.clone()));
    let worker = EnumerationWorker::new(port.clone());
    let pending = worker.start().map_err(|e| format!("{e:?}"))?;
    started.recv_timeout(Duration::from_secs(2))?;
    port.release(1);
    match pending.wait(Duration::from_secs(2)) {
        WaitResult::Ready(value) => {
            assert_eq!(value.mounts, Err(failure));
            assert_eq!(value.capabilities.enumerate.state, CapabilityState::Partial);
        }
        _ => return Err("expected ready".into()),
    }
    assert!(worker.shutdown(Duration::ZERO));
    Ok(())
}

#[test]
fn timed_out_call_keeps_permit_and_discards_late_result() -> Result<(), Box<dyn std::error::Error>>
{
    let (port, started) = fixture(Ok(vec![]));
    let worker = EnumerationWorker::new(port.clone());
    let pending = worker.start().map_err(|e| format!("{e:?}"))?;
    assert_eq!(started.recv_timeout(Duration::from_secs(2))?, 1);
    assert!(matches!(pending.wait(Duration::ZERO), WaitResult::TimedOut));
    assert!(matches!(worker.start(), Err(StartError::Busy)));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    port.release(1);
    // A successful second start proves the abandoned native call released its permit.
    let deadline = Instant::now() + Duration::from_secs(2);
    let second = loop {
        match worker.start() {
            Ok(pending) => break pending,
            Err(StartError::Busy) if Instant::now() < deadline => std::thread::yield_now(),
            _ => return Err("permit was not released".into()),
        }
    };
    assert_eq!(started.recv_timeout(Duration::from_secs(2))?, 2);
    port.release(2);
    assert!(matches!(
        second.wait(Duration::from_secs(2)),
        WaitResult::Ready(_)
    ));
    Ok(())
}

#[test]
fn concurrent_start_admits_only_one_and_drop_abandons_it() -> Result<(), Box<dyn std::error::Error>>
{
    let (port, started) = fixture(Ok(vec![]));
    let worker = Arc::new(EnumerationWorker::new(port.clone()));
    let barrier = Arc::new(std::sync::Barrier::new(9));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let worker = worker.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            worker.start()
        }));
    }
    barrier.wait();
    let mut pending = Vec::new();
    for handle in handles {
        match handle.join().map_err(|_| "starter panicked")? {
            Ok(value) => pending.push(value),
            Err(StartError::Busy) => {}
            Err(error) => return Err(format!("{error:?}").into()),
        }
    }
    assert_eq!(pending.len(), 1);
    assert_eq!(started.recv_timeout(Duration::from_secs(2))?, 1);
    drop(pending);
    assert!(matches!(worker.start(), Err(StartError::Busy)));
    port.release(1);
    assert!(worker.shutdown(Duration::from_secs(2)));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn shutdown_wakes_waiter_rejects_new_work_and_respects_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let (port, started) = fixture(Ok(vec![]));
    let worker = EnumerationWorker::new(port.clone());
    let pending = worker.start().map_err(|e| format!("{e:?}"))?;
    started.recv_timeout(Duration::from_secs(2))?;
    let waiter = std::thread::spawn(move || pending.wait(Duration::from_secs(30)));
    assert!(!worker.shutdown(Duration::ZERO));
    assert!(matches!(worker.start(), Err(StartError::Shutdown)));
    assert!(matches!(
        waiter.join().map_err(|_| "waiter panicked")?,
        WaitResult::Shutdown
    ));
    port.release(1);
    assert!(worker.shutdown(Duration::from_secs(2)));
    assert!(matches!(worker.start(), Err(StartError::Shutdown)));
    Ok(())
}
