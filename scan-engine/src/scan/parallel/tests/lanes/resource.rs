//! Live lane resource changes are enforced at worker assignment boundaries.
use super::*;
use du_core_contracts::analysis_ipc::{ScanResourceMode, ScanSetResourceOutcome};
use du_core_contracts::{
    job::{DeviceSeat, DiskSeat},
    scan_observation::{DeviceKind, DeviceKindSource},
};
use std::collections::HashMap;

#[test]
fn lane_limit_bounds_fake_os_calls_and_live_changes_take_effect_next_assignment()
-> Result<(), TestError> {
    let _serial = worker_test_lock();
    let (first_gate, next_gate) = (Arc::new(Gate::default()), Arc::new(Gate::default()));
    let (_first_open, _next_open) = (Opened(first_gate.clone()), Opened(next_gate.clone()));
    let (first_hook, next_hook, first_count) =
        (first_gate.clone(), next_gate.clone(), AtomicUsize::new(0));
    let probe = Probe::new(
        Shape {
            depth: 1,
            dirs: 1,
            files: 0,
            batch: 1,
        },
        8,
        Box::new(move |_, _, _| {
            if first_count.fetch_add(1, Ordering::SeqCst) < 8 {
                first_hook.wait()?;
            } else {
                next_hook.wait()?;
            }
            Ok(())
        }),
    );
    let shared = shared_for(&probe, vec![0; 8], 4, QUEUED_DIRECTORIES)?;
    let session = Arc::new(ScanControlSession::with_resource_condition(
        8,
        ScanResourceMode::Fast,
        None,
    ));
    session.attach(&shared.queue, &shared.wake);
    let control = session.handle();

    let lane = shared
        .lane_report()
        .and_then(|report| report.lanes.into_iter().next())
        .ok_or("resource lane status missing")?;
    assert_eq!(lane.mode, Some(ScanResourceMode::Fast));
    assert_eq!(lane.permit, Some(16));
    let strategy = lane.strategy.ok_or("resource strategy missing")?;
    assert_eq!(strategy.id, "resource.static");
    assert_eq!(strategy.version, 1);

    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    run_workers_in(&shared, 8, 64, &context, |receiver, sender| {
        drop(sender);
        let observed = (|| -> Result<(), TestError> {
            first_gate.until(|state| state.waiting == 8)?;
            assert_eq!(probe.in_flight.load(Ordering::SeqCst), 8);

            let (outcome, lanes) = control
                .set_resource(ScanResourceMode::Quiet, Some(2))
                .map_err(|error| format!("lower resource limit: {error:?}"))?;
            assert_eq!(outcome, ScanSetResourceOutcome::Accepted);
            assert_eq!(lanes.first().map(|lane| lane.effective_limit), Some(2));
            assert_eq!(lanes.first().and_then(|lane| lane.permit), Some(16));
            assert_eq!(probe.in_flight.load(Ordering::SeqCst), 8);
            assert_eq!(
                first_gate.most(),
                8,
                "in-flight calls must not be interrupted"
            );

            first_gate.release();
            next_gate.until(|state| state.waiting == 2)?;
            assert_eq!(
                next_gate.most(),
                2,
                "lowered lane admitted more than two calls"
            );

            let (outcome, lanes) = control
                .set_resource(ScanResourceMode::Fast, Some(4))
                .map_err(|error| format!("raise resource limit: {error:?}"))?;
            assert_eq!(outcome, ScanSetResourceOutcome::Accepted);
            assert_eq!(lanes.first().map(|lane| lane.effective_limit), Some(4));
            next_gate.until(|state| state.waiting == 4)?;
            assert_eq!(next_gate.most(), 4, "raised lane did not admit four calls");
            Ok(())
        })();
        first_gate.release();
        next_gate.release();
        receiver.iter().for_each(drop);
        observed
    })??;
    assert_eq!(probe.in_flight.load(Ordering::SeqCst), 0);
    Ok(())
}

#[derive(Default)]
struct DiskGate {
    active: Mutex<HashMap<Vec<u8>, usize>>,
    changed: Condvar,
}

impl DiskGate {
    fn try_seat(self: &Arc<Self>, disk: &DiskSeat) -> Option<DeviceSeat> {
        let key = disk.key().to_vec();
        let mut active = self.active.lock().ok()?;
        let count = active.entry(key.clone()).or_default();
        if *count >= du_job_runtime::device_permit(disk.kind()) as usize {
            return None;
        }
        *count += 1;
        drop(active);
        let gate = self.clone();
        Some(DeviceSeat::new(move || {
            if let Ok(mut active) = gate.active.lock()
                && let Some(count) = active.get_mut(&key)
            {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    active.remove(&key);
                }
            }
            gate.changed.notify_all();
        }))
    }
}

struct SeatContext {
    flows: Context,
    disks: Arc<DiskGate>,
}

impl JobContext for SeatContext {
    fn checkpoint(&self) -> Checkpoint {
        self.flows.checkpoint()
    }

    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        self.flows.try_start_flow()
    }

    fn try_device_seat(&self, disk: &DiskSeat) -> Result<Option<DeviceSeat>, JobFlowError> {
        Ok(self.disks.try_seat(disk))
    }

    fn wait_device_seat(&self, disk: &DiskSeat) -> Result<DeviceSeat, JobFlowError> {
        loop {
            if let Some(seat) = self.disks.try_seat(disk) {
                return Ok(seat);
            }
            let active = self.disks.active.lock().map_err(|_| JobFlowError::Closed)?;
            let _ = self
                .disks
                .changed
                .wait_timeout(active, Duration::from_millis(10))
                .map_err(|_| JobFlowError::Closed)?;
            if self.checkpoint() == Checkpoint::Cancel {
                return Err(JobFlowError::Cancelled);
            }
        }
    }
}

#[test]
fn two_hdd_volumes_on_one_disk_never_exceed_four_blocked_calls() -> Result<(), TestError> {
    let _serial = worker_test_lock();
    let gate = Arc::new(Gate::default());
    let _opened = Opened(gate.clone());
    let hook_gate = gate.clone();
    let probe = Probe::new(
        Shape {
            depth: 1,
            dirs: 4,
            files: 0,
            batch: 4,
        },
        2,
        Box::new(move |_, path, call| {
            if call == Call::Read && !path.is_empty() {
                hook_gate.wait()?;
            }
            Ok(())
        }),
    );
    let seat = |kind| crate::scan::parallel::queue::LaneDevice {
        kind,
        source: DeviceKindSource::Detected,
        disk_seat: DiskSeat::new(kind, vec![0x51, 0x52]),
    };
    let shared = shared_over_with_devices(
        probe.roots(),
        vec![0, 1],
        4,
        QUEUED_DIRECTORIES,
        Some(vec![seat(DeviceKind::Hdd), seat(DeviceKind::Hdd)]),
    )?;
    let session = ScanControlSession::with_resource_condition(2, ScanResourceMode::Fast, None);
    session.attach(&shared.queue, &shared.wake);
    let reports = shared.lane_report().ok_or("lane report unavailable")?;
    assert_eq!(reports.lanes.len(), 2);
    for lane in reports.lanes {
        assert_eq!(lane.limit, 2);
        assert_eq!(lane.share.map(std::num::NonZeroU32::get), Some(2));
    }
    let context = SeatContext {
        flows: Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        },
        disks: Arc::new(DiskGate::default()),
    };
    run_workers_in(&shared, 8, 32, &context, |receiver, sender| {
        drop(sender);
        let result = gate
            .until(|state| state.waiting >= 3)
            .map_err(|error| format!("three calls were not concurrently parked: {error}"));
        if result.is_ok() {
            assert!((3..=4).contains(&probe.in_flight.load(Ordering::SeqCst)));
        }
        gate.release();
        receiver.iter().for_each(drop);
        result
    })??;
    assert!((3..=4).contains(&probe.in_flight_peak.load(Ordering::SeqCst)));
    Ok(())
}
