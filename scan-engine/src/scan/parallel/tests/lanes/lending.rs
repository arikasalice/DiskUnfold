//! Workers lent to the marked area's lane (FR8.HD1.app-host.DD1, NFR2.HD1.app-host.DD1).
//!
//! The queue tests drive worker turns by hand: a turn returns the held item to
//! its lane and claims the next one, as `requeue` and `claim_for` do for a
//! worker. Root `r` is in lane `r`.
use super::{
    isolation::{assert_lanes_hold_only_their_roots, frame},
    priority::spawn_scan,
    *,
};
use crate::scan::parallel::{
    lanes::{Origin, Taken},
    queue::PriorityRegion,
};

fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}

/// Replaces the marked area, as an accepted or finished request does.
fn mark(queue: &mut Queue, area: Option<(u32, &[&str])>) {
    let previous = queue.priority.take().map(|region| region.root_index);
    queue.priority = area.map(|(root_index, parts)| PriorityRegion {
        root_index,
        path: path(parts),
    });
    queue.reclassify(previous);
}

fn queue_work(queue: &mut Queue, root: u32, names: &[&[&str]]) {
    for parts in names {
        assert!(queue.push_root(frame(root, path(parts))));
    }
}

/// Ends the turn of `held` by returning its item, then claims the next item.
fn turn(queue: &mut Queue, seat: usize, held: Taken) -> Result<Taken, TestError> {
    let reserved = held.origin == Origin::Ready;
    queue.requeue(seat, held.lane, reserved, held.frame);
    let taken = queue.claim_for(seat).ok_or("no claimable work")?;
    assert_lanes_hold_only_their_roots(queue);
    Ok(taken)
}

/// Ends the turn of `held` with its directory finished, then claims again.
fn finish(queue: &mut Queue, seat: usize, held: Taken) -> Result<Taken, TestError> {
    if held.origin == Origin::Ready {
        queue.lanes.release_reservation(held.lane);
    }
    let taken = queue.claim_for(seat).ok_or("no claimable work")?;
    assert_lanes_hold_only_their_roots(queue);
    Ok(taken)
}

fn lanes_of(queue: &Queue, seats: &[usize]) -> Vec<Option<usize>> {
    seats
        .iter()
        .map(|&seat| queue.lanes.seats[seat].lane)
        .collect()
}

fn members(queue: &Queue) -> Vec<usize> {
    queue.lanes.lanes.iter().map(|lane| lane.members).collect()
}

/// Three workers busy in lane 0; lane 1 holds work but no worker.
fn busy_lane() -> Result<(Queue, Vec<usize>, Vec<Taken>), TestError> {
    let mut queue = Queue::for_test(vec![0, 1], 3);
    let names: Vec<String> = (0..8).map(|index| format!("a{index}")).collect();
    let names: Vec<[&str; 1]> = names.iter().map(|name| [name.as_str()]).collect();
    let names: Vec<&[&str]> = names.iter().map(|name| name.as_slice()).collect();
    queue_work(&mut queue, 0, &names);
    let seats: Vec<usize> = (0..3).map(|_| queue.lanes.register_seat()).collect();
    let held = seats
        .iter()
        .map(|&seat| queue.claim_for(seat).ok_or("no lane 0 work"))
        .collect::<Result<Vec<_>, _>>()?;
    queue_work(
        &mut queue,
        1,
        &[&["c"], &["b", "0"], &["b", "1"], &["b", "2"], &["b", "3"]],
    );
    Ok((queue, seats, held))
}

#[test]
fn busy_workers_move_to_the_marked_lane_after_one_turn_and_return_when_it_ends()
-> Result<(), TestError> {
    let (mut queue, seats, mut held) = busy_lane()?;
    // Without a marked area every worker stays in its lane.
    for (index, &seat) in seats.iter().enumerate() {
        let item = held.remove(0);
        held.push(turn(&mut queue, seat, item)?);
        assert_eq!(held[2].lane, 0, "worker {index}");
    }
    assert_eq!(members(&queue), [3, 0]);
    mark(&mut queue, Some((1, &["b"])));
    // After one more turn each, two workers serve the area; the last worker
    // of lane 0 stays there.
    let mut claimed = Vec::new();
    for &seat in &seats {
        let item = held.remove(0);
        let next = turn(&mut queue, seat, item)?;
        claimed.push((next.lane, next.frame.path.clone()));
        held.push(next);
    }
    assert_eq!(claimed[0], (1, path(&["b", "0"])));
    assert_eq!(claimed[1], (1, path(&["b", "1"])));
    assert_eq!(claimed[2].0, 0);
    assert_eq!(members(&queue), [1, 2]);
    assert_eq!(queue.lanes.seats[seats[0]].home, Some(0));
    // While area work is queued, lent workers stay and take it first.
    let item = held.remove(0);
    let next = turn(&mut queue, seats[0], item)?;
    assert_eq!((next.lane, next.frame.depth()), (1, 2));
    held.push(next);
    // The area ends: a lent worker returns to lane 0 after its turn, and the
    // last worker of lane 1 keeps serving lane 1 until it runs out of work.
    mark(&mut queue, None);
    let item = held.remove(0);
    held.push(turn(&mut queue, seats[1], item)?);
    assert_eq!(held[2].lane, 0);
    assert_eq!(queue.lanes.seats[seats[1]].home, None);
    let item = held.remove(1);
    held.push(turn(&mut queue, seats[0], item)?);
    assert_eq!(held[2].lane, 1);
    assert_eq!(lanes_of(&queue, &seats), [Some(1), Some(0), Some(0)]);
    assert_eq!(members(&queue), [2, 1]);
    Ok(())
}

/// Lanes with a free worker seat and work a joining worker could claim.
fn joinable(queue: &Queue) -> Vec<usize> {
    (0..queue.lanes.lanes.len())
        .filter(|&lane| {
            let state = &queue.lanes.lanes[lane];
            queue.lanes.free_seats(lane) > 0
                && (!state.ready.is_empty() || queue.lanes.stacked(lane) != 0)
        })
        .collect()
}

/// A move frees a seat in the lane it leaves without waking anyone. In the
/// order where the move comes between the area lane's wake and the woken
/// worker's claim, the woken worker takes that seat and no lane is left with
/// both a free seat and claimable work.
///
/// This drives the queue directly: each step stands for a worker turn, and
/// the wakes are the ones the comments name. It does not run `notify_one`,
/// the wake count of `handoff::requeue` or the timed wait of
/// `dispatch::take`, so it fixes the queue state after this order, not that
/// a sleeping thread is woken. See the module documentation of
/// `lanes::lending` for the bound on a missed wake.
#[test]
fn a_move_between_a_wake_and_the_woken_claim_leaves_no_work_unclaimed() -> Result<(), TestError> {
    let mut queue = Queue::for_test(vec![0, 1], 2);
    queue_work(&mut queue, 0, &[&["a0"], &["a1"], &["a2"]]);
    let seats: Vec<usize> = (0..4).map(|_| queue.lanes.register_seat()).collect();
    let (s0, s1, s2, s3) = (seats[0], seats[1], seats[2], seats[3]);
    let a0 = queue.claim_for(s0).ok_or("no lane 0 work")?;
    let _a1 = queue.claim_for(s1).ok_or("no lane 0 work")?;
    // Worker 0 holds a child on its stack; leaving lane 0 returns it there.
    queue.stack(s0, 0, frame(0, path(&["a0", "s"])));
    queue_work(&mut queue, 1, &[&["b"]]);
    let b = queue.claim_for(s2).ok_or("no lane 1 work")?;
    assert_eq!((b.lane, members(&queue)), (1, vec![2, 1]));
    // Lane 0 is full and lane 1 has a seat but nothing to claim: worker 3
    // finds nothing and waits.
    assert!(queue.claim_for(s3).is_none());
    assert!(joinable(&queue).is_empty());
    // The area is marked (the request wakes every worker; nothing changes).
    mark(&mut queue, Some((1, &[])));
    assert!(queue.claim_for(s3).is_none());
    // Worker 2 returns the marked item. Lane 1 has one free seat, so the
    // return wakes one waiting worker: worker 3.
    queue.requeue(s2, b.lane, b.origin == Origin::Ready, b.frame);
    assert_eq!(queue.lanes.free_seats(1), 1);
    assert_eq!(joinable(&queue), [1]);
    // Before worker 3 claims, worker 0 ends its turn and moves to lane 1.
    // The move frees a lane 0 seat and returns the stacked child to lane 0
    // without a wake of its own.
    let moved = turn(&mut queue, s0, a0)?;
    assert_eq!(moved.lane, 1);
    assert_eq!(queue.lanes.moves.lent, 1);
    assert_eq!(joinable(&queue), [0]);
    // The woken worker 3 finds lane 1 full and takes the freed lane 0 seat.
    let woken = queue.claim_for(s3).ok_or("woken worker found nothing")?;
    assert_eq!(woken.lane, 0);
    // Worker 2 lost its item to worker 0 and finds nothing; it waits while
    // no lane holds work that a waiting worker could join.
    assert!(queue.claim_for(s2).is_none());
    assert!(joinable(&queue).is_empty());
    assert_eq!(members(&queue), [2, 1]);
    assert_lanes_hold_only_their_roots(&queue);
    Ok(())
}

#[test]
fn a_marked_lane_at_its_limit_takes_no_worker_until_one_leaves() -> Result<(), TestError> {
    let mut queue = Queue::for_test(vec![0, 1], 2);
    queue_work(&mut queue, 0, &[&["a0"], &["a1"], &["a2"], &["a3"]]);
    queue_work(&mut queue, 1, &[&["c0"], &["c1"], &["b", "0"], &["b", "1"]]);
    let seats: Vec<usize> = (0..4).map(|_| queue.lanes.register_seat()).collect();
    let mut held = seats
        .iter()
        .map(|&seat| queue.claim_for(seat).ok_or("no work"))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(members(&queue), [2, 2]);
    mark(&mut queue, Some((1, &["b"])));
    let before = lanes_of(&queue, &seats);
    for (index, &seat) in seats.iter().enumerate() {
        let item = held.remove(0);
        held.push(turn(&mut queue, seat, item)?);
        assert_eq!(
            queue.lanes.seats[seat].lane, before[index],
            "worker {index}"
        );
    }
    assert_eq!(members(&queue), [2, 2]);
    // A lane 1 worker stops; lane 1 has room again.
    let leaving = seats
        .iter()
        .position(|&seat| queue.lanes.seats[seat].lane == Some(1))
        .ok_or("no lane 1 worker")?;
    let stopped = held.remove(leaving);
    let reserved = stopped.origin == Origin::Ready;
    queue.requeue(seats[leaving], stopped.lane, reserved, stopped.frame);
    queue.lanes.release_seat(seats[leaving]);
    let mover = seats
        .iter()
        .position(|&seat| queue.lanes.seats[seat].lane == Some(0))
        .ok_or("no lane 0 worker")?;
    let item = held.remove(if mover < leaving { mover } else { mover - 1 });
    let next = turn(&mut queue, seats[mover], item)?;
    assert_eq!(next.lane, 1);
    assert_eq!(next.frame.path.first(), Some(&b"b".to_vec()));
    assert_eq!(members(&queue), [1, 2]);
    Ok(())
}

#[test]
fn a_worker_moves_only_when_its_stack_fits_in_its_lane_queue() -> Result<(), TestError> {
    // Lane queues of eight slots; two busy workers in lane 0.
    let mut queue = Queue::for_test_with_capacity(vec![0, 1], 2, 8);
    queue_work(&mut queue, 0, &[&["a0"], &["a1"]]);
    let seats: Vec<usize> = (0..2).map(|_| queue.lanes.register_seat()).collect();
    let mut held = seats
        .iter()
        .map(|&seat| queue.claim_for(seat).ok_or("no work"))
        .collect::<Result<Vec<_>, _>>()?;
    queue_work(&mut queue, 0, &[&["a2"]]);
    // Worker 0 stacked seven children that did not fit in the lane queue.
    for index in 0..7 {
        let name = format!("s{index}");
        queue.stack(seats[0], 0, frame(0, path(&["a0", &name])));
    }
    queue_work(&mut queue, 1, &[&["b", "0"], &["b", "1"]]);
    mark(&mut queue, Some((1, &["b"])));
    // Worker 0 finishes its directory. One queued item, worker 1's reserved
    // slot and seven stacked items exceed the eight slots: it stays and
    // continues its stack.
    let item = held.remove(0);
    let next = finish(&mut queue, seats[0], item)?;
    assert_eq!((next.lane, next.origin), (0, Origin::Stack));
    // After one more finished directory its six stacked items fit exactly:
    // they go back to the lane 0 queue and the worker serves the area.
    let next = finish(&mut queue, seats[0], next)?;
    assert_eq!(next.lane, 1);
    assert_eq!(next.frame.path.first(), Some(&b"b".to_vec()));
    let lane = &queue.lanes.lanes[0];
    assert_eq!(lane.ready.len() + lane.reserved(), 8);
    assert_eq!(queue.lanes.stacked(0), 0);
    // Worker 1 is now lane 0's last worker and stays, though lane 1 has room.
    let item = held.remove(0);
    assert_eq!(finish(&mut queue, seats[1], item)?.lane, 0);
    Ok(())
}

#[test]
fn a_stalled_marked_lane_never_takes_the_last_worker_of_another_lane() -> Result<(), TestError> {
    let _guard = worker_test_lock();
    let shape = Shape {
        depth: 2,
        dirs: 4,
        files: 2,
        batch: 2,
    };
    let (stall, hold) = (Arc::new(Gate::default()), Arc::new(Gate::default()));
    let (_stall, _hold) = (Opened(stall.clone()), Opened(hold.clone()));
    let (stalled, held) = (stall.clone(), hold.clone());
    // Reads below root 0 stall until the end; reads below root 1 wait until
    // the area is marked, so lane 1 cannot finish before the request.
    let probe = Probe::new(
        shape,
        2,
        Box::new(move |root, path, call| {
            if call == Call::Read && !path.is_empty() {
                if root == 0 {
                    stalled.wait()?;
                } else {
                    held.wait()?;
                }
            }
            Ok(())
        }),
    );
    let context = Arc::new(Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    });
    let (handle, collector) = spawn_scan(&probe, vec![0, 1], 4, 4, context);
    stall.until(|state| state.entered >= 1)?;
    hold.until(|state| state.entered >= 1)?;
    let outcome = handle.prioritize(0, Vec::new());
    assert!(
        matches!(outcome, Ok(ScanPriorityOutcome::Accepted { .. })),
        "{outcome:?}"
    );
    hold.release();
    // Lane 1 finishes while every directory below root 0 stays unread.
    wait_until(|| probe.finished(1) == shape.directories())?;
    assert!(probe.finished(0) < shape.directories());
    assert!(stall.most() <= 4, "{}", stall.most());
    stall.release();
    let run = collector.join().map_err(|_| "collector panicked")??;
    assert_eq!(run.completeness, ResultCompleteness::Complete);
    assert_eq!(run.rows.len(), 2 * shape.items());
    Ok(())
}
