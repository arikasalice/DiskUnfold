use super::*;
use std::sync::mpsc;

fn sample(in_flight: u32, blocked_sends: u32, members: usize, queued: usize) -> LaneSample {
    LaneSample {
        in_flight,
        blocked_sends,
        members,
        limit: 2,
        queued,
    }
}

#[test]
fn the_first_matching_evidence_names_the_wait() {
    assert_eq!(sample(1, 1, 2, 3).wait(), (LaneWait::IoWait, true));
    assert_eq!(sample(0, 1, 2, 3).wait(), (LaneWait::Backpressure, true));
    assert_eq!(sample(0, 0, 2, 3).wait(), (LaneWait::DeviceCap, true));
    // An idle lane without work waits for nothing.
    assert_eq!(sample(0, 0, 0, 0).wait(), (LaneWait::None, true));
    // Queued work below the limit, or workers between calls, have no evidence.
    assert_eq!(sample(0, 0, 1, 3).wait(), (LaneWait::None, false));
    assert_eq!(sample(0, 0, 1, 0).wait(), (LaneWait::None, false));
}

#[test]
fn a_bound_call_counts_on_its_lane_until_it_returns() {
    let meters = LaneMeters::new(2);
    let binding = bind(&meters, 1);
    let during = os_call(|| meters.counts(1));
    assert_eq!(during, (1, 0, 0));
    failed();
    assert_eq!(meters.counts(1), (0, 0, 1));
    drop(binding);
    // Unbound work counts nowhere, and an unknown lane reads as zero.
    failed();
    assert_eq!(os_call(|| meters.counts(1)), (0, 0, 1));
    assert_eq!(meters.counts(5), (0, 0, 0));
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
        while watcher.counts(0).1 == 0 {
            std::thread::yield_now();
        }
        receiver.recv().ok()
    });
    send(&sender, 2).map_err(|error| error.to_string())?;
    let after = meters.counts(0);
    let drained = drain.join().map_err(|_| "drain panicked")?;
    assert_eq!((after, drained), ((0, 0, 0), Some(1)));
    Ok(())
}
