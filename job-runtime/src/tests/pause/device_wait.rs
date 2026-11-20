use super::*;
use du_core_contracts::job::{DeviceSeat, DiskSeat, JobFlowError};
use du_core_contracts::scan_observation::DeviceKind;
use std::sync::atomic::{AtomicUsize, Ordering};

struct SequenceIdentity(AtomicUsize);

impl JobIdentity for SequenceIdentity {
    fn next_id(&self) -> Id {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = self.0.fetch_add(1, Ordering::Relaxed).to_le_bytes()[0];
        Id::from_v7_bytes(bytes)
    }
}

struct HeldBody {
    context: mpsc::Sender<Arc<dyn JobContext>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl JobBody for HeldBody {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let _ = self.context.send(context);
        if let Ok(release) = self.release.lock() {
            let _ = release.recv();
        }
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

fn wait_until_parked(runtime: &JobRuntime) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while runtime.flow_limits.waiters.load(Ordering::Acquire) == 0 {
        if Instant::now() >= deadline {
            return Err("device waiter did not park".into());
        }
        std::thread::yield_now();
    }
    Ok(())
}

fn disk(key: &[u8]) -> DiskSeat {
    DiskSeat::new(DeviceKind::Hdd, key.to_vec())
}

#[test]
fn one_runtime_shares_a_disk_limit_across_jobs_and_keys() -> Result<(), Box<dyn std::error::Error>>
{
    let runtime = Arc::new(JobRuntime::new_with_limits(
        Arc::new(SequenceIdentity(AtomicUsize::new(1))),
        Arc::new(Clock),
        Arc::new(History::default()),
        Arc::new(Events::default()),
        FlowLimits {
            metadata_io: 8,
            queued_items: 8,
            item_bytes: 32,
        },
    )?);
    runtime.register(descriptor())?;

    let (first_context_send, first_context_receive) = mpsc::channel();
    let (first_release, first_wait) = mpsc::channel();
    runtime.submit(Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(HeldBody {
            context: first_context_send,
            release: Mutex::new(first_wait),
        }),
        priority: JobPriority::Normal,
        scope_summary: "first".into(),
        scope_roots: Vec::new(),
        correlation_id: Id::from_v7_bytes([2; 16]),
        sample: false,
    })?;
    let first_runtime = runtime.clone();
    let first_worker = std::thread::spawn(move || first_runtime.run_next());
    let first = first_context_receive.recv_timeout(Duration::from_secs(2))?;

    let (second_context_send, second_context_receive) = mpsc::channel();
    let (second_release, second_wait) = mpsc::channel();
    runtime.submit(Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(HeldBody {
            context: second_context_send,
            release: Mutex::new(second_wait),
        }),
        priority: JobPriority::Normal,
        scope_summary: "second".into(),
        scope_roots: Vec::new(),
        correlation_id: Id::from_v7_bytes([3; 16]),
        sample: false,
    })?;
    let second_runtime = runtime.clone();
    let second_worker = std::thread::spawn(move || second_runtime.run_next());
    let second = second_context_receive.recv_timeout(Duration::from_secs(2))?;

    let shared = disk(&[9, 8, 7]);
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(
            first
                .try_device_seat(&shared)
                .map_err(|error| format!("{error:?}"))?
                .ok_or("seat not admitted")?,
        );
    }
    assert!(
        second
            .try_device_seat(&shared)
            .map_err(|error| format!("{error:?}"))?
            .is_none()
    );
    let other = second
        .try_device_seat(&disk(&[6, 5, 4]))
        .map_err(|error| format!("{error:?}"))?
        .ok_or("independent disk was blocked")?;
    drop(other);

    let waiter_context = second.clone();
    let waiter_disk = shared.clone();
    let (waiter_send, waiter_receive) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let _ = waiter_send.send(waiter_context.wait_device_seat(&waiter_disk));
    });
    wait_until_parked(&runtime)?;
    drop(held.pop());
    let released = waiter_receive
        .recv_timeout(Duration::from_secs(2))?
        .map_err(|error| format!("{error:?}"))?;
    waiter.join().map_err(|_| "device waiter panicked")?;
    drop(released);
    drop(held);

    first_release.send(())?;
    second_release.send(())?;
    first_worker.join().map_err(|_| "first job panicked")??;
    second_worker.join().map_err(|_| "second job panicked")??;
    Ok(())
}

fn spawn_device_waiter(
    context: Arc<dyn JobContext>,
    disk: DiskSeat,
) -> (
    std::thread::JoinHandle<()>,
    mpsc::Receiver<Result<DeviceSeat, JobFlowError>>,
) {
    let (send, receive) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let _ = send.send(context.wait_device_seat(&disk));
    });
    (waiter, receive)
}

#[test]
fn pause_cancel_abort_and_close_wake_disk_seat_waiters() -> Result<(), Box<dyn std::error::Error>> {
    for action in ["pause", "cancel", "abort", "close"] {
        let (runtime, _, id, context, release, worker) =
            setup_with_resource(true, ResourceClass::MetadataIo, 0)?;
        let shared = disk(&[3, 2, 1]);
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(
                context
                    .try_device_seat(&shared)
                    .map_err(|error| format!("{error:?}"))?
                    .ok_or("seat not admitted")?,
            );
        }
        let (waiter, result) = spawn_device_waiter(context.clone(), shared);
        wait_until_parked(&runtime)?;
        match action {
            "pause" => {
                runtime.pause(&id)?;
            }
            "cancel" => {
                runtime.cancel(&id)?;
            }
            "abort" => context.abort_flows(),
            _ => {
                release.send(())?;
            }
        }
        let received = result.recv_timeout(Duration::from_secs(2))?;
        match (action, received) {
            ("pause", Err(JobFlowError::Busy))
            | ("cancel", Err(JobFlowError::Cancelled))
            | ("abort" | "close", Err(JobFlowError::Closed)) => {}
            (_, Ok(_)) => return Err("disk wait acquired a seat unexpectedly".into()),
            (_, Err(error)) => {
                return Err(format!("unexpected disk wait error: {error:?}").into());
            }
        }
        waiter.join().map_err(|_| "device waiter panicked")?;
        drop(held);
        if action == "pause" {
            runtime.resume(&id)?;
        }
        if action != "close" {
            release.send(())?;
        }
        worker.join().map_err(|_| "job worker panicked")??;
    }
    Ok(())
}
