//! NFR3.HD1.app-host.DD1: assignment cost over a 100 000-directory tree.
//!
//! One thread drives a lane for 1, 4 and 8 worker seats as the workers do:
//! claim one directory, offer its children to the lane queue, stack those that
//! do not fit and release the claimed slot. The index key comparisons, items
//! inspected and time of each claim are grouped by the work held in the lane
//! when the claim was made. Run with `--nocapture` to print the table.
//!
//! The tree has one root, so each depth's root index holds one key: a claim
//! from the lane queue compares at most three keys, one per root-index search,
//! and a claim from a stack compares none. With several roots a search compares
//! keys along one B-tree path, whose length grows with the logarithm of the
//! distinct roots queued at that depth (see the lanes module).
use super::*;
use crate::scan::parallel::lanes::{Origin, cost};

const DIRECTORIES: usize = 100_000;
const FANOUT: usize = 10;
/// Exclusive upper bounds of the held-work groups, and their labels.
const GROUPS: [usize; 4] = [64, 1_000, 10_000, usize::MAX];
const LABELS: [&str; 4] = ["0-63", "64-999", "1000-9999", "10000+"];

#[derive(Clone, Copy, Default)]
struct Group {
    claims: u64,
    compared: u64,
    most_compared: u64,
    inspected: u64,
    most_inspected: u64,
    nanos: u128,
}

struct Unopened;
impl DirectoryObservations for Unopened {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for Unopened {
    fn observed_identity(&self) -> Option<HandleIdentity> {
        None
    }
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(io_error("test.unused"))
    }
}

/// Directory `n` of the tree has children `n * FANOUT + 1 ..`; its name is `n`.
fn directory(path: Vec<Vec<u8>>) -> Frame {
    let mut frame = Frame::root(0, Box::new(Unopened), Boundary::Unknown);
    frame.root = path.is_empty();
    frame.path = path;
    frame
}

fn children(frame: &Frame) -> Vec<Frame> {
    let node: usize = frame
        .path
        .last()
        .and_then(|name| std::str::from_utf8(name).ok()?.parse().ok())
        .unwrap_or(0);
    let first = node * FANOUT + 1;
    (first..(first + FANOUT).min(DIRECTORIES))
        .map(|child| {
            let mut path = frame.path.clone();
            path.push(child.to_string().into_bytes());
            directory(path)
        })
        .collect()
}

/// Assigns every directory of the tree once and returns the claim costs.
fn drive(workers: usize, capacity: usize) -> [Group; GROUPS.len()] {
    let mut queue = Queue::for_test_with_capacity(vec![0], workers, capacity);
    let seats: Vec<usize> = (0..workers).map(|_| queue.lanes.register_seat()).collect();
    assert!(queue.push_root(directory(Vec::new())));
    let mut groups = [Group::default(); GROUPS.len()];
    let (mut claimed, mut idle, mut turn) = (0, 0, 0);
    while claimed < DIRECTORIES {
        let seat = seats[turn % workers];
        turn += 1;
        let held = queue.lanes.queued() + queue.lanes.stacked(0);
        cost::take();
        let started = Instant::now();
        let taken = queue.claim_for(seat);
        let nanos = started.elapsed().as_nanos();
        let (compared, inspected) = cost::take();
        let Some(taken) = taken else {
            idle += 1;
            assert!(idle < workers, "no seat could claim held work");
            continue;
        };
        idle = 0;
        claimed += 1;
        let index = GROUPS.iter().position(|&bound| held < bound).unwrap_or(0);
        let group = &mut groups[index];
        group.claims += 1;
        group.compared += compared;
        group.most_compared = group.most_compared.max(compared);
        group.inspected += inspected;
        group.most_inspected = group.most_inspected.max(inspected);
        group.nanos += nanos;
        for child in children(&taken.frame) {
            if let Some(child) = queue.offer(taken.lane, child) {
                queue.stack(seat, taken.lane, child);
            }
        }
        if taken.origin == Origin::Ready {
            queue.lanes.release_reservation(taken.lane);
        }
    }
    assert_eq!(queue.lanes.queued() + queue.lanes.stacked(0), 0);
    groups
}

#[test]
fn claim_cost_does_not_grow_with_held_work() {
    for capacity in [QUEUED_DIRECTORIES, DIRECTORIES] {
        for workers in [1, 4, 8] {
            let groups = drive(workers, capacity);
            let used: Vec<(usize, &str, Group)> = GROUPS
                .iter()
                .copied()
                .zip(LABELS)
                .zip(groups)
                .map(|((bound, label), group)| (bound, label, group))
                .filter(|(_, _, group)| group.claims > 0)
                .collect();
            for (_, label, group) in &used {
                let claims = group.claims as f64;
                eprintln!(
                    "capacity={capacity} workers={workers} held={label}: claims={} \
                     compared avg={:.2} max={} inspected avg={:.2} max={} ns avg={:.0}",
                    group.claims,
                    group.compared as f64 / claims,
                    group.most_compared,
                    group.inspected as f64 / claims,
                    group.most_inspected,
                    group.nanos as f64 / claims,
                );
            }
            // A claim pops one item, or reads one stack head per other worker
            // of the lane when it steals. With one root, a claim makes at most
            // three root-index searches of one key each, whatever is held.
            let steal = workers.saturating_sub(1).max(1) as u64;
            let mut largest = 0;
            for (bound, label, group) in &used {
                assert!(group.most_inspected <= steal, "{workers}/{label}");
                assert!(group.most_compared <= 3, "{workers}/{label}");
                largest = largest.max(*bound);
            }
            if capacity == DIRECTORIES {
                // The unbounded lane queue held more than 10 000 items.
                assert_eq!(largest, usize::MAX, "{workers}");
            }
        }
    }
}
