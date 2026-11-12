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
