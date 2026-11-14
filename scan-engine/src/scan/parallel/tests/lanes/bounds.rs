//! A stopped consumer stalls collection within fixed bounds; tight limits
//! never deadlock (NFR3.HD1.app-host.DD1).
use super::*;
use std::sync::mpsc::TrySendError;

/// Lane state read under the queue lock once collection has stalled.
struct Snapshot {
    /// Items in each lane queue plus the slots reserved by claimed items.
    held_slots: Vec<usize>,
    queued: usize,
    stacked: usize,
    /// Returned entries that queued or stacked frames still hold.
    held_entries: usize,
    /// Entries the OS returned, read under the same lock.
    returned: usize,
    /// Probe events that the test itself put in the channel while waiting.
    probes: usize,
}

fn snapshot(queue: &Queue, probe: &Probe, probes: usize) -> Snapshot {
    let lanes = &queue.lanes;
    let stacked_frames = lanes.seats.iter().flat_map(|seat| seat.stack.frames());
    let queued_frames = lanes.lanes.iter().flat_map(|lane| lane.ready.frames());
    Snapshot {
        held_slots: lanes
            .lanes
            .iter()
            .map(|lane| lane.ready.len() + lane.reserved())
            .collect(),
        queued: lanes.queued(),
        stacked: (0..lanes.lanes.len()).map(|lane| lanes.stacked(lane)).sum(),
        held_entries: queued_frames
            .chain(stacked_frames)
            .map(|frame| frame.entries.len())
            .sum(),
        returned: probe.returned.load(Ordering::SeqCst),
        probes,
    }
}

/// Waits until collection is stalled on the full observation channel and
/// returns the lane state read under the queue lock at that moment.
///
/// Stalled means, all at once: no open or read is in flight, every worker
/// holds a claimed directory, at least `floor` returned entries have left
/// their frames, and the channel is full, so a worker's next send blocks. The
/// fullness check is a `try_send` on a clone of the workers' sender; the
/// events it adds while the channel still had room are counted in the result.
fn wait_stalled(
    shared: &Shared,
    probe: &Probe,
    workers: usize,
    floor: usize,
    sender: &SyncSender<Event>,
) -> Result<Snapshot, TestError> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut probes = 0;
    loop {
        {
            let queue = shared.queue.lock().map_err(|_| "queue lock")?;
            let current = snapshot(&queue, probe, probes);
            let in_hand = queue.remaining - current.queued - current.stacked;
            let left = current.returned - current.held_entries;
            if probe.in_flight.load(Ordering::SeqCst) == 0 && in_hand == workers && left >= floor {
                match sender.try_send(Event::RootBatch(0, None, None)) {
                    Err(TrySendError::Full(_)) => return Ok(current),
                    Ok(()) => probes += 1,
                    Err(TrySendError::Disconnected(_)) => return Err("channel closed".into()),
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("collection did not stall".into());
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_stopped_consumer_bounds_observations_queued_work_and_open_handles() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let shape = Shape {
        depth: 3,
        dirs: 6,
        files: 40,
        batch: 8,
    };
    let (roots, lanes, workers) = (2, 2, 4);
    let probe = Probe::quiet(shape, roots);
    let shared = shared_for(&probe, vec![0, 1], 4, QUEUED_DIRECTORIES)?;
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    // Entries out of their frames when the channel is nearly full: every
    // buffered event but the root batches, less what the workers hold.
    let floor = BUFFERED_OBSERVATIONS - roots - workers * (shape.batch + 1);
    let (stalled, items) = run_workers_in(
        &shared,
        workers,
        BUFFERED_OBSERVATIONS,
        &context,
        |receiver, sender| {
            // Nothing is received until collection has stalled on the full channel.
            let stalled = wait_stalled(&shared, &probe, workers, floor, &sender);
            drop(sender);
            let items = receiver
                .iter()
                .filter(|event| matches!(event, Event::Item(..)))
                .count();
            (stalled, items)
        },
    )?;
    let snapshot = stalled?;
    // Entries the OS returned beyond those still held by queued or stacked
    // frames are in the full channel, minus at most two root batch events and
    // the test's own probe events, or held by a worker, which owns at most one
    // frame's batch.
    let unconsumed = snapshot.returned - snapshot.held_entries;
    assert!(
        unconsumed + snapshot.probes >= BUFFERED_OBSERVATIONS - roots,
        "{unconsumed}"
    );
    assert!(
        unconsumed <= BUFFERED_OBSERVATIONS + workers * (shape.batch + 1),
        "{unconsumed}"
    );
    // Queued work stays within the lane capacity; overflow stacks within the depth.
    assert!(
        snapshot
            .held_slots
            .iter()
            .all(|&slots| slots <= QUEUED_DIRECTORIES),
        "{:?}",
        snapshot.held_slots
    );
    assert!(
        snapshot.stacked <= workers * (shape.depth + 2),
        "{}",
        snapshot.stacked
    );
    // A handle is held by its own frame or shared by pending children.
    let frames = snapshot.queued + snapshot.stacked + workers;
    let live = probe.live.load(Ordering::SeqCst);
    assert!(
        live <= 2 * frames + roots,
        "{live} handles for {frames} frames"
    );
    let bound = 2 * (lanes * QUEUED_DIRECTORIES + workers * (shape.depth + 3)) + roots;
    assert!(probe.peak.load(Ordering::SeqCst) <= bound);
    let queue = shared.queue.lock().map_err(|_| "queue lock")?;
    assert!(
        queue
            .lanes
            .peak()
            .is_some_and(|peak| peak <= lanes * QUEUED_DIRECTORIES)
    );
    // After the consumer resumes, collection completes without losing items.
    assert_eq!(items, roots * (shape.items() - 1));
    assert_eq!(queue.remaining, 0);
    assert!(!shared.abort.load(Ordering::SeqCst));
    assert_eq!(probe.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn lanes_at_their_smallest_limits_finish_without_deadlock() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let shape = Shape {
        depth: 4,
        dirs: 3,
        files: 2,
        batch: 1,
    };
    // (lane queue capacity, channel capacity, workers, worker limit per lane)
    for (capacity, channel, workers, limit) in [(1, 1, 8, 2), (2, 3, 3, 1), (1, 16, 1, 1)] {
        let probe = Probe::quiet(shape, 3);
        let shared = shared_for(&probe, vec![0, 1, 0], limit, capacity)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let items = run_workers(&shared, workers, channel, |receiver| {
            let mut items = 0;
            loop {
                match receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(Event::Item(..)) => items += 1,
                    Ok(_) => {}
                    Err(RecvTimeoutError::Disconnected) => return Ok(items),
                    Err(RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                    Err(RecvTimeoutError::Timeout) => {
                        shared.abort.store(true, Ordering::SeqCst);
                        shared.wake.notify_all();
                        return Err(format!("stalled after {items} items"));
                    }
                }
            }
        })??;
        assert_eq!(items, 3 * (shape.items() - 1), "capacity {capacity}");
        assert!(!shared.abort.load(Ordering::SeqCst));
        assert_eq!(shared.queue.lock().map_err(|_| "queue lock")?.remaining, 0);
        assert_eq!(probe.live.load(Ordering::SeqCst), 0);
    }
    Ok(())
}
