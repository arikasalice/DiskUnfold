use super::super::super::queue as scheduler_queue;
use super::*;

fn frame(root_index: u32, path: Vec<Vec<u8>>) -> Frame {
    Frame {
        root_index,
        path,
        directory: Box::new(PriorityChild {
            visited: true,
            label: "unused",
            gate: None,
            visits: mpsc::channel().0,
        }),
        entries: VecDeque::new(),
        deferred: VecDeque::new(),
        root: false,
        first_batch: false,
        root_published: false,
        batch_pending: false,
        case_observed: false,
        boundary: Boundary::Unknown,
    }
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
    let mut queue = Queue {
        ready: paths
            .iter()
            .map(|(root, path)| frame(*root, path.clone()))
            .collect(),
        ready_high_water: None,
        last_root: None,
        priority: None,
        outstanding: Some({
            let mut outstanding = scheduler_queue::OutstandingPaths::new(2);
            for (root, path) in &paths {
                assert!(outstanding.register(*root, path));
            }
            outstanding
        }),
        remaining: paths.len(),
        root_remaining: vec![4, 1],
        root_groups: vec![0, 1],
        active_groups: vec![0, 0],
        device_limit: 2,
        started: true,
    };
    assert_eq!(
        queue
            .pop_fair()
            .map(|(frame, _)| (frame.root_index, frame.path)),
        Some((0, Vec::new()))
    );
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
    assert_eq!(
        queue
            .pop_fair()
            .map(|(frame, _)| (frame.root_index, frame.path)),
        Some((0, vec![b"B".to_vec()]))
    );
    assert_eq!(queue.priority_rank(&frame(0, Vec::new())), 1);
    assert_eq!(queue.priority_rank(&queue.ready[0]), 2);
    assert_eq!(queue.priority_rank(&queue.ready[1]), 2);
    assert_eq!(queue.priority_rank(&queue.ready[2]), 2);
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

type TestPath = (u32, Vec<Vec<u8>>);

/// Builds a two-root queue whose roots are active and whose `ready` frames
/// keep the given order. Every registered path counts as outstanding work.
fn live_queue(registered: &[TestPath], ready: &[TestPath]) -> Queue {
    let mut outstanding = scheduler_queue::OutstandingPaths::new(2);
    for (root, path) in registered {
        assert!(outstanding.register(*root, path));
    }
    Queue {
        ready: ready
            .iter()
            .map(|(root, path)| frame(*root, path.clone()))
            .collect(),
        ready_high_water: None,
        last_root: None,
        priority: None,
        outstanding: Some(outstanding),
        remaining: registered.len(),
        root_remaining: vec![registered.len(), 1],
        root_groups: vec![0, 1],
        active_groups: vec![1, 1],
        device_limit: 8,
        started: true,
    }
}

fn drain(queue: &mut Queue) -> Vec<TestPath> {
    std::iter::from_fn(|| {
        queue
            .pop_fair()
            .map(|(frame, _)| (frame.root_index, frame.path))
    })
    .collect()
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

    let mut queue = live_queue(&registered, &ready);
    assert!(matches!(
        queue.prioritize(scheduler_queue::PriorityRegion {
            root_index: 0,
            path: target,
        }),
        scheduler_queue::PriorityDecision::Accepted { remaining: 2 }
    ));
    let order = drain(&mut queue);
    assert_eq!(order.first(), Some(&(0, inside)));

    // The remaining frames follow the unprioritized fair order from the same
    // scheduler state: neither root 1's "B" nor the "B2" sibling is promoted.
    let mut fair = live_queue(&registered, &lookalikes);
    fair.last_root = Some(0);
    assert_eq!(order[1..], drain(&mut fair)[..]);
    assert_eq!(order[1], (1, vec![b"B".to_vec()]));
}

#[test]
fn running_area_without_queued_work_keeps_its_priority_marker() {
    let area = vec![b"R".to_vec()];
    let child = vec![b"R".to_vec(), b"C".to_vec()];
    let sibling = vec![b"S".to_vec()];
    // "R" is running: it is outstanding, but no frame below it is queued.
    let mut queue = live_queue(
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
    assert_eq!(queue.best_ready_rank(), Some(2));
    assert_eq!(queue.priority.as_ref(), Some(&region()));

    // The running area discovers a child after the request; it is assigned first.
    assert!(
        queue
            .outstanding
            .as_mut()
            .is_some_and(|outstanding| outstanding.register(0, &child))
    );
    queue.push_ready(frame(0, child.clone()));
    assert_eq!(
        queue
            .pop_fair()
            .map(|(frame, _)| (frame.root_index, frame.path)),
        Some((0, child.clone()))
    );

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
