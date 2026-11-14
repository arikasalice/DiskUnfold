use super::super::super::queue as scheduler_queue;
use super::*;

fn frame(root_index: u32, path: Vec<Vec<u8>>) -> Frame {
    let mut frame = Frame::root(
        root_index,
        Box::new(PriorityChild {
            visited: true,
            label: "unused",
            gate: None,
            visits: mpsc::channel().0,
        }),
    );
    frame.root = false;
    frame.path = path;
    frame
}

type TestPath = (u32, Vec<Vec<u8>>);

/// Builds one lane holding two roots. Every registered path counts as
/// outstanding work and `ready` frames enter the lane queue in order.
fn live_queue(registered: &[TestPath], ready: &[TestPath]) -> (Queue, usize) {
    let mut outstanding = scheduler_queue::OutstandingPaths::new(2);
    for (root, path) in registered {
        assert!(outstanding.register(*root, path));
    }
    let mut queue = Queue::for_test(vec![0, 0], 8);
    queue.outstanding = Some(outstanding);
    queue.remaining = registered.len();
    queue.root_remaining = vec![registered.len(), 1];
    for (root, path) in ready {
        assert!(queue.push_root(frame(*root, path.clone())));
    }
    let seat = queue.lanes.register_seat();
    (queue, seat)
}

/// Claims the next item as the lane's worker would.
fn pop(queue: &mut Queue, seat: usize) -> Option<TestPath> {
    queue
        .claim_for(seat)
        .map(|taken| (taken.frame.root_index, taken.frame.path))
}

fn drain(queue: &mut Queue, seat: usize) -> Vec<TestPath> {
    std::iter::from_fn(|| pop(queue, seat)).collect()
}

#[test]
fn replacement_and_root_component_boundaries_preserve_fair_order() {
    let paths = [
        (0, Vec::<Vec<u8>>::new()),
        (0, vec![b"A".to_vec()]),
        (0, vec![b"B".to_vec()]),
        (1, vec![b"B".to_vec()]),
        (0, vec![b"B2".to_vec()]),
    ];
    let (mut queue, seat) = live_queue(&paths, &paths);
    assert_eq!(pop(&mut queue, seat), Some((0, Vec::new())));
    assert!(matches!(
        queue.prioritize(scheduler_queue::PriorityRegion {
            root_index: 0,
            path: vec![b"A".to_vec()],
        }),
        scheduler_queue::PriorityDecision::Accepted { remaining: 1 }
    ));
    assert!(matches!(
        queue.prioritize(scheduler_queue::PriorityRegion {
            root_index: 0,
            path: vec![b"B".to_vec()],
        }),
        scheduler_queue::PriorityDecision::Accepted { remaining: 1 }
    ));
    assert!(matches!(
        queue.prioritize(scheduler_queue::PriorityRegion {
            root_index: 0,
            path: vec![b"B".to_vec()],
        }),
        scheduler_queue::PriorityDecision::NoEffect {
            same_region: true,
            remaining: 1,
        }
    ));
    assert_eq!(pop(&mut queue, seat), Some((0, vec![b"B".to_vec()])));
    assert_eq!(queue.priority_rank(&frame(0, Vec::new())), 1);
    for (root, path) in &paths[1..] {
        if path != &[b"B".to_vec()] || *root != 0 {
            assert_eq!(queue.priority_rank(&frame(*root, path.clone())), 2);
        }
    }
    // The replaced area no longer marks "A"; every queued item is normal.
    assert_eq!(queue.lanes.lanes[0].ready.best_class(), Some(2));
    assert!(queue.complete_path(0, &[b"B".to_vec()]));
    assert_eq!(queue.priority, None);
    assert!(matches!(
        queue.prioritize(scheduler_queue::PriorityRegion {
            root_index: 0,
            path: vec![b"B".to_vec()],
        }),
        scheduler_queue::PriorityDecision::NoEffect {
            same_region: false,
            remaining: 0,
        }
    ));
}

#[test]
fn deep_outstanding_paths_are_released_without_recursive_stack_use() {
    let mut outstanding = scheduler_queue::OutstandingPaths::new(1);
    let path = vec![vec![b'x']; 32_768];

    assert!(outstanding.register(0, &[]));
    assert!(outstanding.register(0, &path));
    assert_eq!(outstanding.count(0, &[]), 2);
    assert_eq!(outstanding.count(0, &path), 1);

    assert!(outstanding.complete(0, &path));
    assert_eq!(outstanding.count(0, &path), 0);
    assert!(outstanding.complete(0, &[]));
    assert_eq!(outstanding.count(0, &[]), 0);
}

#[test]
fn other_roots_and_component_prefix_lookalikes_keep_fair_order() {
    let target = vec![b"B".to_vec()];
    let inside = vec![b"B".to_vec(), b"C".to_vec()];
    let lookalikes: Vec<TestPath> = vec![
        (1, target.clone()),
        (0, vec![b"B2".to_vec()]),
        (0, vec![b"A".to_vec()]),
    ];
    let mut ready = lookalikes.clone();
    ready.push((0, inside.clone()));
    let mut registered = vec![(0, Vec::new()), (1, Vec::new()), (0, target.clone())];
    registered.extend(ready.iter().cloned());

    let (mut queue, seat) = live_queue(&registered, &ready);
    assert!(matches!(
        queue.prioritize(scheduler_queue::PriorityRegion {
            root_index: 0,
            path: target,
        }),
        scheduler_queue::PriorityDecision::Accepted { remaining: 2 }
    ));
    let order = drain(&mut queue, seat);
    assert_eq!(order.first(), Some(&(0, inside)));

    // The remaining frames follow the unprioritized fair order from the same
    // scheduler state: neither root 1's "B" nor the "B2" sibling is promoted.
    let (mut fair, seat) = live_queue(&registered, &lookalikes);
    fair.lanes.lanes[0].last_root = Some(0);
    assert_eq!(order[1..], drain(&mut fair, seat)[..]);
    assert_eq!(order[1], (1, vec![b"B".to_vec()]));
}

#[test]
fn running_area_without_queued_work_keeps_its_priority_marker() {
    let area = vec![b"R".to_vec()];
    let child = vec![b"R".to_vec(), b"C".to_vec()];
    let sibling = vec![b"S".to_vec()];
    // "R" is running: it is outstanding, but no frame below it is queued.
    let (mut queue, seat) = live_queue(
        &[(0, Vec::new()), (0, area.clone()), (0, sibling.clone())],
        &[(0, sibling.clone())],
    );
    let region = || scheduler_queue::PriorityRegion {
        root_index: 0,
        path: area.clone(),
    };
    assert!(matches!(
        queue.prioritize(region()),
        scheduler_queue::PriorityDecision::Accepted { remaining: 1 }
    ));
    assert_eq!(queue.lanes.lanes[0].ready.best_class(), Some(2));
    assert_eq!(queue.priority.as_ref(), Some(&region()));

    // The running area discovers a child after the request; it is assigned first.
    assert!(
        queue
            .outstanding
            .as_mut()
            .is_some_and(|outstanding| outstanding.register(0, &child))
    );
    assert!(queue.offer(0, frame(0, child.clone())).is_none());
    assert_eq!(pop(&mut queue, seat), Some((0, child.clone())));

    // The area itself finishing does not end the region while its child runs.
    assert!(queue.complete_path(0, &area));
    assert_eq!(queue.priority.as_ref(), Some(&region()));
    assert!(matches!(
        queue.prioritize(region()),
        scheduler_queue::PriorityDecision::NoEffect {
            same_region: true,
            remaining: 1,
        }
    ));
    assert!(queue.complete_path(0, &child));
    assert_eq!(queue.priority, None);
    assert!(matches!(
        queue.prioritize(region()),
        scheduler_queue::PriorityDecision::NoEffect {
            same_region: false,
            remaining: 0,
        }
    ));
}
