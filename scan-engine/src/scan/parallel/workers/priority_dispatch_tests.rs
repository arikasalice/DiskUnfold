//! Claim order inside a lane: class, own stack, depth, root turns, stealing.
use super::*;
use crate::scan::parallel::lanes::{Origin, cost};
use du_core_contracts::scan_observation::DirectoryObservations;

struct EmptyDirectory;

impl DirectoryObservations for EmptyDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}

impl AnchoredDirectory for EmptyDirectory {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Ok(Box::new(Self))
    }
}

fn frame(root: u32, path: &[&[u8]]) -> Frame {
    let mut frame = Frame::root(root, Box::new(EmptyDirectory));
    frame.root = false;
    frame.path = path.iter().map(|part| part.to_vec()).collect();
    frame
}

fn region(root: u32, path: &[&[u8]]) -> queue::PriorityRegion {
    queue::PriorityRegion {
        root_index: root,
        path: path.iter().map(|part| part.to_vec()).collect(),
    }
}

/// A queue whose seat 0 is a member of lane 0.
fn member_queue(groups: Vec<usize>, limit: usize) -> (Queue, usize) {
    let mut queue = Queue::for_test(groups, limit);
    let seat = queue.lanes.register_seat();
    queue.lanes.join(seat, 0);
    (queue, seat)
}

fn claim(queue: &mut Queue, seat: usize) -> Option<(u32, Vec<Vec<u8>>, Origin)> {
    queue
        .claim_for(seat)
        .map(|taken| (taken.frame.root_index, taken.frame.path, taken.origin))
}

fn path(parts: &[&[u8]]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.to_vec()).collect()
}

#[test]
fn lower_class_wins_across_lane_queue_and_own_stack() {
    let (mut queue, seat) = member_queue(vec![0], 4);
    queue.priority = Some(region(0, &[b"B"]));
    assert!(queue.push_root(frame(0, &[b"A"])));
    queue.stack(seat, 0, frame(0, &[b"B"]));
    assert_eq!(
        claim(&mut queue, seat),
        Some((0, path(&[b"B"]), Origin::Stack))
    );

    let (mut queue, seat) = member_queue(vec![0], 4);
    queue.priority = Some(region(0, &[b"B"]));
    assert!(queue.push_root(frame(0, &[b"B"])));
    queue.stack(seat, 0, frame(0, &[b"A"]));
    assert_eq!(
        claim(&mut queue, seat),
        Some((0, path(&[b"B"]), Origin::Ready))
    );
}

#[test]
fn equal_class_continues_the_own_stack_newest_first() {
    let (mut queue, seat) = member_queue(vec![0], 4);
    queue.priority = Some(region(0, &[b"B"]));
    assert!(queue.push_root(frame(0, &[b"B"])));
    queue.stack(seat, 0, frame(0, &[b"B", b"C", b"D"]));
    assert_eq!(
        claim(&mut queue, seat),
        Some((0, path(&[b"B", b"C", b"D"]), Origin::Stack))
    );

    // Without an area, a deeper own item still precedes shallower lane work,
    // which keeps the overflow stack a depth-first chain.
    let (mut queue, seat) = member_queue(vec![0], 4);
    assert!(queue.push_root(frame(0, &[b"A"])));
    queue.stack(seat, 0, frame(0, &[b"X"]));
    queue.stack(seat, 0, frame(0, &[b"X", b"Y"]));
    assert_eq!(
        claim(&mut queue, seat),
        Some((0, path(&[b"X", b"Y"]), Origin::Stack))
    );
    assert_eq!(claim(&mut queue, seat), Some((0, path(&[b"X"]), Origin::Stack)));
    assert_eq!(claim(&mut queue, seat), Some((0, path(&[b"A"]), Origin::Ready)));
}

#[test]
fn lane_queue_takes_area_then_ancestors_then_shallow_depth_with_root_turns() {
    let (mut queue, seat) = member_queue(vec![0, 0], 4);
    for (root, parts) in [
        (0, vec![&b"a"[..], b"b", b"c"]),
        (0, vec![&b"a"[..], b"b"]),
        (1, vec![&b"y"[..], b"z"]),
        (0, vec![&b"a"[..]]),
        (0, vec![&b"q"[..]]),
        (1, vec![&b"x"[..]]),
    ] {
        assert!(queue.push_root(frame(root, &parts)));
    }
    let mut unmarked = Vec::new();
    while let Some((root, parts, _)) = claim(&mut queue, seat) {
        unmarked.push((root, parts.len()));
    }
    // Shallow first; within one depth the lane's roots take turns.
    assert_eq!(unmarked, [(0, 1), (1, 1), (0, 1), (1, 2), (0, 2), (0, 3)]);

    let (mut queue, seat) = member_queue(vec![0, 0], 4);
    for (root, parts) in [
        (1, vec![&b"x"[..]]),
        (0, vec![&b"q"[..]]),
        (0, vec![&b"a"[..], b"b", b"c"]),
        (0, vec![&b"a"[..], b"b"]),
        (0, vec![&b"a"[..]]),
    ] {
        assert!(queue.push_root(frame(root, &parts)));
    }
    queue.priority = Some(region(0, &[b"a", b"b", b"c"]));
    queue.reclassify(None);
    let order: Vec<_> = std::iter::from_fn(|| claim(&mut queue, seat))
        .map(|(root, parts, _)| (root, parts))
        .collect();
    assert_eq!(
        order,
        [
            (0, path(&[b"a", b"b", b"c"])),
            (0, path(&[b"a"])),
            (0, path(&[b"a", b"b"])),
            (1, path(&[b"x"])),
            (0, path(&[b"q"])),
        ]
    );
}

#[test]
fn idle_worker_steals_only_by_joining_the_same_lane_within_its_limit() {
    let mut queue = Queue::for_test(vec![0, 1], 2);
    let owner = queue.lanes.register_seat();
    let idle = queue.lanes.register_seat();
    queue.lanes.join(owner, 0);
    queue.lanes.join(idle, 1);
    queue.stack(owner, 0, frame(0, &[b"s"]));
    queue.stack(owner, 0, frame(0, &[b"s", b"t"]));

    // Lane 1 has nothing; the idle worker becomes a lane 0 worker and steals
    // the oldest, shallowest item from the stack of that lane's worker.
    assert_eq!(claim(&mut queue, idle), Some((0, path(&[b"s"]), Origin::Stack)));
    assert_eq!(queue.lanes.seats[idle].lane, Some(0));
    assert_eq!(queue.lanes.lanes[1].members, 0);
    assert_eq!(queue.lanes.stacked(0), 1);

    // A full lane admits no other worker, so its work stays with its members.
    let mut queue = Queue::for_test(vec![0, 1], 1);
    let owner = queue.lanes.register_seat();
    let idle = queue.lanes.register_seat();
    queue.lanes.join(owner, 0);
    queue.stack(owner, 0, frame(0, &[b"s"]));
    assert_eq!(claim(&mut queue, idle), None);
    assert_eq!(queue.lanes.seats[idle].lane, None);
    assert_eq!(queue.lanes.stacked(0), 1);

    // A worker with work in its own lane never takes another lane's work.
    let mut queue = Queue::for_test(vec![0, 1], 2);
    let worker = queue.lanes.register_seat();
    queue.lanes.join(worker, 1);
    queue.priority = Some(region(0, &[b"hot"]));
    assert!(queue.push_root(frame(0, &[b"hot"])));
    assert!(queue.push_root(frame(1, &[b"cold"])));
    assert_eq!(claim(&mut queue, worker), Some((1, path(&[b"cold"]), Origin::Ready)));
    assert_eq!(queue.lanes.seats[worker].lane, Some(1));
}

/// Fills one worker stack, and the lane queue if `queued`, with `count` items.
///
/// Items of 64 roots share first components `g0` to `g15`, so an area on
/// one component of one root holds about `count / 1024` items of each kind.
fn crowded(count: usize, queued: bool) -> (Queue, usize) {
    let (mut queue, seat) = member_queue(vec![0; 64], 4);
    for index in 0..count {
        let root = (index % 64) as u32;
        let depth = 1 + index % 8;
        let parts: Vec<Vec<u8>> = std::iter::once(format!("g{}", (index / 64) % 16))
            .chain((1..depth).map(|level| format!("{}-{level}", index / 64)))
            .map(String::into_bytes)
            .collect();
        if queued {
            let mut item = frame(root, &[]);
            item.path = parts.clone();
            assert!(queue.push_root(item));
        }
        let mut stacked = frame(root, &[]);
        stacked.path = parts;
        queue.stack(seat, 0, stacked);
    }
    (queue, seat)
}

/// Claims `count` items, alternating with a second worker of the lane, and
/// returns the worst (index key comparisons, items inspected) of one claim.
fn worst_claim_cost(queue: &mut Queue, seat: usize, count: usize) -> (u64, u64) {
    let mut worst = (0, 0);
    let other = queue.lanes.register_seat();
    for turn in 0..count {
        let claimer = if turn % 2 == 0 { seat } else { other };
        cost::take();
        assert!(queue.claim_for(claimer).is_some());
        let (compared, inspected) = cost::take();
        worst = (worst.0.max(compared), worst.1.max(inspected));
    }
    worst
}

#[test]
fn marked_claims_and_steals_do_not_visit_queued_work() {
    let mut costs = Vec::new();
    for count in [1_000, 40_000] {
        let (mut queue, seat) = crowded(count, true);
        // The area holds the items of root 3 below "g7"; the other items of
        // root 3 stay normal and the 63 other roots are not visited.
        queue.priority = Some(region(3, &[b"g7"]));
        cost::take();
        queue.reclassify(None);
        let (_, reclassified) = cost::take();
        assert!(queue.lanes.lanes[0].ready.best_class() == Some(0));
        let claims = worst_claim_cost(&mut queue, seat, 400);
        // A worker with an empty stack in a lane with an empty queue steals.
        let (mut queue, _) = crowded(count, false);
        queue.priority = Some(region(3, &[b"g7"]));
        queue.reclassify(None);
        let thief = queue.lanes.register_seat();
        let mut steals = (0, 0);
        for _ in 0..200 {
            cost::take();
            let taken = queue.claim_for(thief).map(|taken| taken.origin);
            assert_eq!(taken, Some(Origin::Stack));
            let (compared, inspected) = cost::take();
            steals = (steals.0.max(compared), steals.1.max(inspected));
            queue.lanes.release_seat(thief);
        }
        costs.push((count, reclassified, claims, steals));
    }
    for &(count, reclassified, claims, steals) in &costs {
        // A claim inspects one item; a steal also reads one class head per
        // other worker of the lane. Key comparisons stay bounded.
        assert!(claims.1 <= 1, "{count}: claim inspected {} items", claims.1);
        assert!(claims.0 <= 48, "{count}: claim compared {} keys", claims.0);
        assert!(steals.1 <= 2, "{count}: steal inspected {} items", steals.1);
        assert_eq!(steals.0, 0, "{count}: steal compared keys");
        // Marking is one pass: the marked root's lane-queue items and one pass
        // over the worker stack, never the other roots' lane-queue items.
        assert!(reclassified as usize <= count + count / 64 + 2, "{count}: {reclassified}");
    }
    // Forty times more queued work does not raise the cost of one claim.
    assert!(costs[1].2.0 <= costs[0].2.0 + 8, "{costs:?}");
    assert_eq!(costs[1].2.1, costs[0].2.1);
}
