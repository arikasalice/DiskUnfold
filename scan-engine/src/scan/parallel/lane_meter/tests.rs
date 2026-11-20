use super::*;
use std::sync::mpsc;

fn sample(
    device_cap_waiters: u32,
    in_flight: u32,
    blocked_sends: u32,
    members: usize,
    queued: usize,
) -> LaneSample {
    LaneSample {
        device_cap_waiters,
        in_flight,
        blocked_sends,
        members,
        limit: 2,
        queued,
    }
}

#[test]
fn the_first_matching_evidence_names_the_wait() {
    assert_eq!(sample(0, 1, 1, 2, 3).wait(), (LaneWait::IoWait, true));
    assert_eq!(sample(0, 0, 1, 2, 3).wait(), (LaneWait::Backpressure, true));
    assert_eq!(sample(0, 0, 0, 2, 3).wait(), (LaneWait::DeviceCap, true));
    assert_eq!(sample(1, 0, 0, 2, 3).wait(), (LaneWait::DeviceCap, true));
    // An idle lane without work waits for nothing.
    assert_eq!(sample(0, 0, 0, 0, 0).wait(), (LaneWait::None, true));
    // Queued work below the limit, or workers between calls, have no evidence.
    assert_eq!(sample(0, 0, 0, 1, 3).wait(), (LaneWait::None, false));
    assert_eq!(sample(0, 0, 0, 1, 0).wait(), (LaneWait::None, false));
}

#[test]
fn a_bound_call_counts_on_its_lane_until_it_returns() {
    let meters = LaneMeters::new(2);
    let binding = bind(&meters, 1);
    let during = os_call(|| meters.counts(1));
    assert_eq!(during, (0, 1, 0, 0));
    failed();
    assert_eq!(meters.counts(1), (0, 0, 0, 1));
    drop(binding);
    // Unbound work counts nowhere, and an unknown lane reads as zero.
    failed();
    assert_eq!(os_call(|| meters.counts(1)), (0, 0, 0, 1));
    assert_eq!(meters.counts(5), (0, 0, 0, 0));
}

#[test]
fn a_parked_disk_permit_wait_reports_a_confirmed_device_cap() -> Result<(), String> {
    let meters = LaneMeters::new(1);
    let worker_meters = Arc::clone(&meters);
    let (started, started_wait) = mpsc::channel();
    let (release, release_wait) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _binding = bind(&worker_meters, 0);
        device_wait(|| {
            let _ = started.send(());
            let _ = release_wait.recv();
        });
    });
    started_wait
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|error| error.to_string())?;
    let (device_cap_waiters, in_flight, blocked_sends, failures) = meters.counts(0);
    assert_eq!(
        LaneSample {
            device_cap_waiters,
            in_flight,
            blocked_sends,
            members: 1,
            limit: 1,
            queued: 1,
        }
        .wait(),
        (LaneWait::DeviceCap, true)
    );
    assert_eq!(failures, 0);
    release.send(()).map_err(|error| error.to_string())?;
    worker.join().map_err(|_| "device wait worker panicked")?;
    Ok(())
}

#[test]
fn a_send_to_a_full_channel_counts_as_blocked_until_room_frees() -> Result<(), String> {
    let meters = LaneMeters::new(1);
    let (sender, receiver) = mpsc::sync_channel(1);
    let _binding = bind(&meters, 0);
    send(&sender, 1).map_err(|error| error.to_string())?;
    let watcher = Arc::clone(&meters);
    let drain = std::thread::spawn(move || {
        // Wait until the second send is counted as blocked, then make room.
        while watcher.counts(0).2 == 0 {
            std::thread::yield_now();
        }
        // The receiver is returned so the channel stays open until the blocked
        // send has completed and the test joins this thread.
        (receiver.recv().ok(), receiver)
    });
    send(&sender, 2).map_err(|error| error.to_string())?;
    let after = meters.counts(0);
    let (drained, _receiver) = drain.join().map_err(|_| "drain panicked")?;
    assert_eq!((after, drained), ((0, 0, 0, 0), Some(1)));
    Ok(())
}
