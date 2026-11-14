//! A stalled lane keeps its own workers, and no directory work changes lanes.
use super::*;
use crate::scan::parallel::lanes::{Origin, Taken};

fn context() -> Context {
    Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    }
}

#[test]
fn a_stalled_lane_keeps_its_workers_while_another_lane_finishes() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let shape = Shape {
        depth: 2,
        dirs: 4,
        files: 3,
        batch: 2,
    };
    let gate = Arc::new(Gate::default());
    let _open = Opened(gate.clone());
    let blocked = gate.clone();
    // Every read below root 0 waits for the gate: lane 0 stops making progress.
    let probe = Probe::new(
        shape,
        2,
        Box::new(move |root, path, call| {
            if root == 0 && call == Call::Read && !path.is_empty() {
                blocked.wait()?;
            }
            Ok(())
        }),
    );
    let job_probe = probe.clone();
    let collector = thread::spawn(move || run_forest(&job_probe, vec![0, 1], 4, 2, &context()));
    wait_until(|| probe.finished(1) == shape.directories())?;
    // Both lane 0 workers are stuck; the two others finished lane 1 and could
    // not join lane 0, which is at its limit of two workers.
    gate.until(|state| state.waiting == 2)?;
    thread::sleep(Duration::from_millis(100));
    assert_eq!(gate.most(), 2);
    assert!(probe.finished(0) < shape.directories());
    gate.release();
    let run = collector.join().map_err(|_| "collector panicked")??;
    assert_eq!(run.completeness, ResultCompleteness::Complete);
    assert_eq!(run.rows.len(), 2 * shape.items());
    assert!(run.issues.is_empty(), "{:?}", run.issues);
    Ok(())
}

pub(super) fn frame(root: u32, path: Vec<Vec<u8>>) -> Frame {
    struct Unused;
    impl DirectoryObservations for Unused {
        fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
            Ok(Vec::new())
        }
    }
    impl AnchoredDirectory for Unused {
        fn observed_identity(&self) -> Option<HandleIdentity> {
            None
        }
        fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
            Err(io_error("test.unused"))
        }
    }
    let mut frame = Frame::root(root, Box::new(Unused), Boundary::Unknown);
    frame.root = path.is_empty();
    frame.path = path;
    frame
}

/// Every queued or stacked item lies in the lane of its root.
pub(super) fn assert_lanes_hold_only_their_roots(queue: &Queue) {
    for (lane, state) in queue.lanes.lanes.iter().enumerate() {
        for item in state.ready.frames() {
            assert_eq!(queue.lanes.lane_of(item.root_index), Some(lane));
        }
        assert!(state.ready.len() + state.reserved() <= QUEUED_DIRECTORIES);
    }
    for seat in &queue.lanes.seats {
        let roots: Vec<_> = seat.stack.frames().map(|item| item.root_index).collect();
        match seat.lane {
            Some(lane) => assert!(
                roots
                    .iter()
                    .all(|&root| queue.lanes.lane_of(root) == Some(lane))
            ),
            None => assert!(roots.is_empty()),
        }
    }
}

/// One step of a worker that holds `taken`: return it, with an optional new
/// child that may not fit in the lane queue, or finish it.
fn finish_turn(queue: &mut Queue, seat: usize, taken: Taken, choice: u64) {
    let reserved = taken.origin == Origin::Ready;
    let Taken {
        frame: item, lane, ..
    } = taken;
    match choice % 3 {
        0 if item.depth() < 6 => {
            let mut path = item.path.clone();
            path.push(format!("c{choice}").into_bytes());
            let child = queue.offer(lane, frame(item.root_index, path));
            queue.requeue(seat, lane, reserved, item);
            if let Some(child) = child {
                queue.stack(seat, lane, child);
            }
        }
        1 => queue.requeue(seat, lane, reserved, item),
        _ if reserved => queue.lanes.release_reservation(lane),
        _ => {}
    }
}

#[test]
fn directory_work_stays_in_the_lane_of_its_root() {
    let groups = vec![0, 1, 2, 0, 1, 2];
    let mut queue = Queue::for_test(groups.clone(), 2);
    let seats: Vec<_> = (0..5).map(|_| queue.lanes.register_seat()).collect();
    let mut held: Vec<Option<Taken>> = seats.iter().map(|_| None).collect();
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..4 {
        for root in 0..groups.len() as u32 {
            assert!(queue.push_root(frame(root, Vec::new())));
        }
    }
    let mut claims = 0;
    for step in 0..30_000 {
        if step % 500 == 0 {
            // New work arrives through the bounded path, as found children do.
            for root in 0..groups.len() as u32 {
                if let Some(lane) = queue.lanes.lane_of(root) {
                    let _ = queue.offer(lane, frame(root, vec![b"new".to_vec()]));
                }
            }
        }
        let pick = (next() % seats.len() as u64) as usize;
        let seat = seats[pick];
        match held[pick].take() {
            Some(taken) => finish_turn(&mut queue, seat, taken, next()),
            None => {
                if let Some(taken) = queue.claim_for(seat) {
                    let lane = queue.lanes.lane_of(taken.frame.root_index);
                    assert_eq!(Some(taken.lane), lane);
                    assert_eq!(queue.lanes.seats[seat].lane, lane);
                    claims += 1;
                    held[pick] = Some(taken);
                }
            }
        }
        assert_lanes_hold_only_their_roots(&queue);
        for lane in &queue.lanes.lanes {
            assert!(lane.members <= lane.limit());
        }
    }
    assert!(claims > 5_000, "{claims}");
}
