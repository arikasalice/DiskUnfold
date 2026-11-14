//! Per-storage lanes: a bounded lane queue, worker seats and same-lane stealing.
//!
//! NFR2.HD1.app-host.DD1: every device group of the scope has one lane. Its
//! directory work lives only in that lane's queue or on the overflow stacks of
//! the workers assigned to the lane, and only those workers claim or steal it.
//! A worker belongs to at most one lane at a time and joins another lane only
//! when its own lane has nothing it can claim, by becoming that lane's worker
//! within the lane's limit. A lane whose OS calls stall therefore keeps only
//! the workers already inside it, and its queued work never moves elsewhere.
//!
//! NFR3.HD1.app-host.DD1: a lane queue holds at most `capacity` items, counting
//! items claimed from it that will be returned to it. A newly found child that
//! does not fit stays on the finding worker's stack, which that worker continues
//! depth first, so no push waits for room and a stack stays within the tree
//! depth. Claims read only class heads, one item and logarithmic index keys.
mod ready;
use super::{
    Frame, Queue,
    queue::{PriorityRegion, rank_in},
};
#[cfg(test)]
pub(super) use ready::cost;
pub(super) use ready::{CLASSES, NORMAL};
use ready::{Arrival, ReadyQueue, Stack};
use std::collections::HashMap;

/// Classes that mark the prioritized area and its ancestors.
const MARKED: [usize; NORMAL] = [0, 1];

/// Directory work and worker accounting of one storage lane.
pub(super) struct Lane {
    pub(super) ready: ReadyQueue,
    limit: usize,
    capacity: usize,
    pub(super) members: usize,
    reserved: usize,
    stacked: usize,
    pub(super) last_root: Option<u32>,
}

impl Lane {
    fn new(limit: usize, capacity: usize) -> Self {
        Self {
            ready: ReadyQueue::default(),
            limit,
            capacity,
            members: 0,
            reserved: 0,
            stacked: 0,
            last_root: None,
        }
    }

    /// Effective worker limit, read again on every join.
    ///
    /// A device resource policy can replace this per-lane value without
    /// changing how workers are assigned.
    pub(super) fn limit(&self) -> usize {
        self.limit
    }

    fn has_room(&self) -> bool {
        self.ready.len() + self.reserved < self.capacity
    }

    fn claimable(&self) -> bool {
        !self.ready.is_empty() || self.stacked != 0
    }

    /// Items claimed from the lane queue that hold a slot until returned.
    #[cfg(test)]
    pub(super) fn reserved(&self) -> usize {
        self.reserved
    }
}

/// One worker's lane membership and overflow stack.
#[derive(Default)]
pub(super) struct Seat {
    pub(super) lane: Option<usize>,
    pub(super) stack: Stack,
}

/// Where a claimed item came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    /// The lane queue; the item keeps a reserved slot until it is returned.
    Ready,
    /// A worker stack of the same lane.
    Stack,
}

/// An item claimed by one worker for one turn.
pub(super) struct Taken {
    pub(super) frame: Frame,
    pub(super) lane: usize,
    pub(super) origin: Origin,
}

/// All lanes of one scan and the seats of its workers.
pub(super) struct Lanes {
    pub(super) lanes: Vec<Lane>,
    root_lanes: Vec<usize>,
    pub(super) seats: Vec<Seat>,
    cursor: usize,
    arrivals: u64,
    queued: usize,
    peak: Option<usize>,
}

impl Lanes {
    /// Builds one lane per distinct group; `groups[root]` names a root's group.
    pub(super) fn new(groups: &[usize], limit: usize, capacity: usize, track_peak: bool) -> Self {
        let mut dense: HashMap<usize, usize> = HashMap::new();
        let root_lanes = groups
            .iter()
            .map(|group| {
                let next = dense.len();
                *dense.entry(*group).or_insert(next)
            })
            .collect();
        Self {
            lanes: (0..dense.len())
                .map(|_| Lane::new(limit, capacity))
                .collect(),
            root_lanes,
            seats: Vec::new(),
            cursor: 0,
            arrivals: 0,
            queued: 0,
            peak: track_peak.then_some(0),
        }
    }

    pub(super) fn lane_of(&self, root: u32) -> Option<usize> {
        self.root_lanes.get(root as usize).copied()
    }

    /// Greatest number of items held in all lane queues at once, if tracked.
    pub(super) fn peak(&self) -> Option<usize> {
        self.peak
    }

    /// Items currently held in all lane queues.
    #[cfg(test)]
    pub(super) fn queued(&self) -> usize {
        self.queued
    }

    /// Items currently held on the stacks of the lane's workers.
    #[cfg(test)]
    pub(super) fn stacked(&self, lane: usize) -> usize {
        self.lanes.get(lane).map_or(0, |state| state.stacked)
    }

    pub(super) fn register_seat(&mut self) -> usize {
        self.seats.push(Seat::default());
        self.seats.len() - 1
    }

    /// Leaves the lane and returns any leftover stack items to its queue.
    pub(super) fn release_seat(&mut self, seat: usize) {
        if seat < self.seats.len() {
            self.leave(seat);
        }
    }

    fn next_arrival(&mut self) -> u64 {
        self.arrivals += 1;
        self.arrivals
    }

    fn push_ready(&mut self, lane: usize, class: usize, frame: Frame) {
        let arrival = self.next_arrival();
        self.lanes[lane].ready.push(class, (arrival, frame));
        self.queued += 1;
        if let Some(peak) = &mut self.peak {
            *peak = (*peak).max(self.queued);
        }
    }

    fn push_stack(&mut self, seat: usize, lane: usize, class: usize, frame: Frame) {
        if self.seats[seat].lane != Some(lane) {
            // Only a member may hold a lane's work; never strand it elsewhere.
            self.push_ready(lane, class, frame);
            return;
        }
        let arrival = self.next_arrival();
        self.seats[seat].stack.push(class, (arrival, frame));
        self.lanes[lane].stacked += 1;
    }

    pub(super) fn join(&mut self, seat: usize, lane: usize) {
        self.seats[seat].lane = Some(lane);
        self.lanes[lane].members += 1;
    }

    fn leave(&mut self, seat: usize) {
        let Some(lane) = self.seats[seat].lane.take() else {
            return;
        };
        self.lanes[lane].members -= 1;
        for (class, frame) in self.seats[seat].stack.drain() {
            self.lanes[lane].stacked -= 1;
            self.push_ready(lane, class, frame);
        }
    }

    pub(super) fn release_reservation(&mut self, lane: usize) {
        if let Some(state) = self.lanes.get_mut(lane) {
            state.reserved = state.reserved.saturating_sub(1);
        }
    }

    /// A lane with claimable work and room for one more worker.
    ///
    /// Lanes with fewer workers come first, so workers stuck in a stalled
    /// lane steer new arrivals elsewhere; ties rotate between lanes.
    fn choose(&mut self) -> Option<usize> {
        let count = self.lanes.len();
        let start = self.cursor;
        let lane = (0..count)
            .map(|offset| (start + offset) % count)
            .filter(|&lane| {
                let state = &self.lanes[lane];
                state.members < state.limit() && state.claimable()
            })
            .min_by_key(|&lane| {
                let state = &self.lanes[lane];
                (state.members, state.ready.best_class().unwrap_or(CLASSES))
            })?;
        self.cursor = (lane + 1) % count;
        Some(lane)
    }

    /// Own stack first, unless the lane queue holds a strictly lower class.
    /// With both empty, the worker steals from another worker of this lane.
    fn claim_in(&mut self, seat: usize, lane: usize) -> Option<Taken> {
        let own = self.seats[seat].stack.best_class();
        let queued = self.lanes[lane].ready.best_class();
        match (own, queued) {
            (Some(own), Some(queued)) if queued < own => self.take_ready(lane),
            (Some(_), _) => self.take_own(seat, lane),
            (None, Some(_)) => self.take_ready(lane),
            (None, None) => self.steal(seat, lane),
        }
    }

    fn take_ready(&mut self, lane: usize) -> Option<Taken> {
        let state = &mut self.lanes[lane];
        let frame = state.ready.pop(state.last_root)?;
        state.last_root = Some(frame.root_index);
        state.reserved += 1;
        self.queued -= 1;
        Some(Taken {
            frame,
            lane,
            origin: Origin::Ready,
        })
    }

    fn take_own(&mut self, seat: usize, lane: usize) -> Option<Taken> {
        let frame = self.seats[seat].stack.pop_top()?;
        self.lanes[lane].stacked -= 1;
        Some(Taken {
            frame,
            lane,
            origin: Origin::Stack,
        })
    }

    /// Takes the oldest lowest-class item, preferring the shallowest, from
    /// another worker of the same lane. Only class heads are compared.
    fn steal(&mut self, seat: usize, lane: usize) -> Option<Taken> {
        let (_, _, victim) = self
            .seats
            .iter()
            .enumerate()
            .filter(|(index, other)| *index != seat && other.lane == Some(lane))
            .filter_map(|(index, other)| {
                other
                    .stack
                    .bottom()
                    .map(|(class, depth)| (class, depth, index))
            })
            .min()?;
        let frame = self.seats[victim].stack.pop_bottom()?;
        self.lanes[lane].stacked -= 1;
        Some(Taken {
            frame,
            lane,
            origin: Origin::Stack,
        })
    }

    /// Any held item for cancellation delivery: own stack, lane queues, stacks.
    ///
    /// Drained items only deliver entries that the OS already returned; no
    /// directory is opened or read for them.
    pub(super) fn take_any(&mut self, seat: usize) -> Option<Frame> {
        if let Some(frame) = self.pop_stack_of(seat) {
            return Some(frame);
        }
        for lane in 0..self.lanes.len() {
            if let Some(frame) = self.lanes[lane].ready.pop(None) {
                self.queued -= 1;
                return Some(frame);
            }
        }
        (0..self.seats.len()).find_map(|other| self.pop_stack_of(other))
    }

    fn pop_stack_of(&mut self, seat: usize) -> Option<Frame> {
        let owner = self.seats.get_mut(seat)?;
        let frame = owner.stack.pop_top()?;
        if let Some(lane) = owner.lane {
            self.lanes[lane].stacked -= 1;
        }
        Some(frame)
    }

    /// Returns marked items of `root` to the normal class, in arrival order.
    fn unmark(&mut self, root: u32) {
        let Some(lane) = self.lane_of(root) else {
            return;
        };
        let state = &mut self.lanes[lane];
        let items = state.ready.take_root(&MARKED, root);
        state.ready.restore(NORMAL, items);
        for seat in self.seats.iter_mut().filter(|seat| seat.lane == Some(lane)) {
            let items = seat.stack.take_root(&MARKED, root);
            seat.stack.restore(NORMAL, items);
        }
    }

    /// Classifies the normal items of the region's root; other roots are untouched.
    fn mark(&mut self, region: &PriorityRegion) {
        let Some(lane) = self.lane_of(region.root_index) else {
            return;
        };
        let state = &mut self.lanes[lane];
        let items = state.ready.take_root(&[NORMAL], region.root_index);
        for (class, items) in split(region, items).into_iter().enumerate() {
            state.ready.restore(class, items);
        }
        for seat in self.seats.iter_mut().filter(|seat| seat.lane == Some(lane)) {
            let items = seat.stack.take_root(&[NORMAL], region.root_index);
            for (class, items) in split(region, items).into_iter().enumerate() {
                seat.stack.restore(class, items);
            }
        }
    }
}

fn split(region: &PriorityRegion, items: Vec<Arrival>) -> [Vec<Arrival>; CLASSES] {
    let mut classes: [Vec<Arrival>; CLASSES] = Default::default();
    for item in items {
        let class = rank_in(Some(region), &item.1);
        classes[class.min(NORMAL)].push(item);
    }
    classes
}

impl Queue {
    /// Claims the next item for `seat`, joining or leaving lanes as needed.
    ///
    /// A worker stays in its lane while that lane has claimable work. Without
    /// any, it leaves and may join another lane with claimable work and room.
    pub(super) fn claim_for(&mut self, seat: usize) -> Option<Taken> {
        if !self.started || seat >= self.lanes.seats.len() {
            return None;
        }
        if let Some(lane) = self.lanes.seats[seat].lane {
            if let Some(taken) = self.lanes.claim_in(seat, lane) {
                return Some(taken);
            }
            self.lanes.leave(seat);
        }
        let lane = self.lanes.choose()?;
        self.lanes.join(seat, lane);
        let taken = self.lanes.claim_in(seat, lane);
        if taken.is_none() {
            self.lanes.leave(seat);
        }
        taken
    }

    /// Returns a claimed item after one turn.
    ///
    /// An item claimed from the lane queue goes back to it in its reserved
    /// slot. Any other item goes to the queue if it has room, else back on the
    /// worker's stack.
    pub(super) fn requeue(&mut self, seat: usize, lane: usize, reserved: bool, frame: Frame) {
        let class = self.priority_rank(&frame);
        if reserved {
            self.lanes.release_reservation(lane);
            self.lanes.push_ready(lane, class, frame);
        } else if self.lanes.lanes[lane].has_room() {
            self.lanes.push_ready(lane, class, frame);
        } else {
            self.lanes.push_stack(seat, lane, class, frame);
        }
    }

    /// Queues new work in its lane if there is room; otherwise returns it.
    pub(super) fn offer(&mut self, lane: usize, frame: Frame) -> Option<Frame> {
        if !self.lanes.lanes[lane].has_room() {
            return Some(frame);
        }
        let class = self.priority_rank(&frame);
        self.lanes.push_ready(lane, class, frame);
        None
    }

    /// Puts work that did not fit on the worker's own stack.
    pub(super) fn stack(&mut self, seat: usize, lane: usize, frame: Frame) {
        let class = self.priority_rank(&frame);
        self.lanes.push_stack(seat, lane, class, frame);
    }

    /// Queues a scope root in its lane without applying the capacity bound.
    pub(super) fn push_root(&mut self, frame: Frame) -> bool {
        let Some(lane) = self.lanes.lane_of(frame.root_index) else {
            return false;
        };
        let class = self.priority_rank(&frame);
        self.lanes.push_ready(lane, class, frame);
        true
    }

    /// Moves held items between classes after the marked area changed.
    ///
    /// Runs once per accepted or finished area. In the lane queue only items
    /// of the previous and the current area root are visited; each worker
    /// stack of that root's lane is passed over once. Other lanes and other
    /// roots' queued items are not visited.
    pub(super) fn reclassify(&mut self, previous: Option<u32>) {
        if let Some(root) = previous {
            self.lanes.unmark(root);
        }
        if let Some(region) = &self.priority {
            self.lanes.mark(region);
        }
    }
}
