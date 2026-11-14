//! Workers lent to the lane of the marked area (FR8.HD1.app-host.DD1).
//!
//! Priority classes order work only inside a lane, so a marked area whose lane
//! has fewer workers than its limit could wait while other lanes keep theirs.
//! Before each claim a worker of another lane therefore moves to the area's
//! lane when that lane queues marked work and has room for one more worker.
//!
//! A move never takes directory work out of its lane: the moving worker holds
//! no claimed item between turns, and its overflow stack goes back to its own
//! lane queue, so it moves only when the stack fits in that queue's free slots.
//! It also moves only when another worker stays in its lane, so a stalled area
//! lane cannot take the last worker of any other lane. A lent worker returns
//! to its own lane, under the same conditions, once the lane it serves queues
//! no marked work and its own lane has claimable work and room. Without a
//! marked area, or without queued marked work, no worker moves this way.
use super::{Lanes, NORMAL, Taken};
use crate::scan::parallel::Queue;

impl Lanes {
    /// The lane of the area root `root` while its lane queue holds marked work.
    fn marked_lane(&self, root: Option<u32>) -> Option<usize> {
        let lane = self.lane_of(root?)?;
        self.lanes[lane]
            .ready
            .best_class()
            .is_some_and(|class| class < NORMAL)
            .then_some(lane)
    }

    fn has_seat(&self, lane: usize) -> bool {
        let state = &self.lanes[lane];
        state.members < state.limit()
    }

    /// Whether `seat` may leave `lane` before the lane runs out of work.
    ///
    /// An empty stack always fits: scope roots may fill a lane queue beyond
    /// its capacity, and a worker that returns nothing must still be able to go.
    fn can_leave(&self, seat: usize, lane: usize) -> bool {
        let state = &self.lanes[lane];
        let stacked = self.seats[seat].stack.len();
        let fits = stacked == 0 || state.ready.len() + state.reserved + stacked <= state.capacity;
        state.members > 1 && fits
    }

    /// Moves `seat` to the marked lane or back home; returns the lane it serves.
    fn lend_or_return(&mut self, seat: usize, lane: usize, marked: Option<usize>) -> usize {
        let home = self.seats[seat].home;
        let (to, home) = match marked {
            Some(area) if area == lane => return lane,
            Some(area) if self.has_seat(area) => (area, home.or(Some(lane))),
            _ => match home {
                Some(home)
                    if home != lane && self.has_seat(home) && self.lanes[home].claimable() =>
                {
                    (home, None)
                }
                _ => return lane,
            },
        };
        if !self.can_leave(seat, lane) {
            return lane;
        }
        // Leaving returns the stack to this lane's queue; it fits (see above).
        self.leave(seat);
        self.join(seat, to);
        self.seats[seat].home = home.filter(|&home| home != to);
        to
    }
}

impl Queue {
    /// Claims the next item for `seat`, joining or leaving lanes as needed.
    ///
    /// A worker stays in its lane while that lane has claimable work, unless it
    /// is lent to the marked area's lane or returns from it (see the module
    /// documentation). Without claimable work it leaves and joins the marked
    /// area's lane if that lane queues marked work and has room, otherwise
    /// another lane with claimable work and room.
    pub(in crate::scan::parallel) fn claim_for(&mut self, seat: usize) -> Option<Taken> {
        if !self.started || seat >= self.lanes.seats.len() {
            return None;
        }
        let root = self.priority.as_ref().map(|region| region.root_index);
        if let Some(lane) = self.lanes.seats[seat].lane {
            let marked = self.lanes.marked_lane(root);
            let lane = self.lanes.lend_or_return(seat, lane, marked);
            if let Some(taken) = self.lanes.claim_in(seat, lane) {
                return Some(taken);
            }
            self.lanes.leave(seat);
        }
        let lane = self
            .lanes
            .marked_lane(root)
            .filter(|&lane| self.lanes.has_seat(lane))
            .or_else(|| self.lanes.choose())?;
        self.lanes.join(seat, lane);
        let taken = self.lanes.claim_in(seat, lane);
        if taken.is_none() {
            self.lanes.leave(seat);
        }
        taken
    }
}
