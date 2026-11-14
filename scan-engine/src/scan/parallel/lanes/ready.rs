//! Lane queue indexes whose claim cost does not grow with queued work.
//!
//! Work is kept per priority class (inside the marked area, its ancestors,
//! everything else). A lane queue orders each class by depth, then takes roots
//! in turn, then arrival. A worker stack keeps each class in arrival order.
//! Claiming touches one item and a logarithmic number of index keys; only a
//! change of the marked area moves items between classes.
use super::super::Frame;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, VecDeque},
    ops::Bound,
};

/// Number of priority classes; lower classes are claimed first.
pub(in crate::scan::parallel) const CLASSES: usize = 3;
/// The class of every item while no area is marked.
pub(in crate::scan::parallel) const NORMAL: usize = CLASSES - 1;

/// An item with the order in which it entered its current queue or stack.
pub(in crate::scan::parallel) type Arrival = (u64, Frame);

/// Work-proportional cost counters, kept per thread for tests only.
#[cfg(test)]
pub(in crate::scan::parallel) mod cost {
    use std::cell::Cell;
    thread_local! {
        static COMPARED: Cell<u64> = const { Cell::new(0) };
        static INSPECTED: Cell<u64> = const { Cell::new(0) };
    }
    pub(in crate::scan::parallel) fn compared() {
        COMPARED.with(|cell| cell.set(cell.get() + 1));
    }
    pub(in crate::scan::parallel) fn inspected(count: usize) {
        INSPECTED.with(|cell| cell.set(cell.get() + count as u64));
    }
    /// Returns and clears (index key comparisons, items inspected).
    pub(in crate::scan::parallel) fn take() -> (u64, u64) {
        (COMPARED.with(|c| c.replace(0)), INSPECTED.with(|c| c.replace(0)))
    }
}

#[inline]
fn compared() {
    #[cfg(test)]
    cost::compared();
}

#[inline]
fn inspected(count: usize) {
    #[cfg(test)]
    cost::inspected(count);
    #[cfg(not(test))]
    let _ = count;
}

/// Depth index key; comparisons are counted in tests.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Depth(usize);
impl Ord for Depth {
    fn cmp(&self, other: &Self) -> Ordering {
        compared();
        self.0.cmp(&other.0)
    }
}
impl PartialOrd for Depth {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Root index key; comparisons are counted in tests.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Root(u32);
impl Ord for Root {
    fn cmp(&self, other: &Self) -> Ordering {
        compared();
        self.0.cmp(&other.0)
    }
}
impl PartialOrd for Root {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

type Tier = BTreeMap<Depth, BTreeMap<Root, VecDeque<Arrival>>>;

/// The shared queue of one lane.
#[derive(Default)]
pub(in crate::scan::parallel) struct ReadyQueue {
    tiers: [Tier; CLASSES],
    len: usize,
}

impl ReadyQueue {
    pub(in crate::scan::parallel) fn len(&self) -> usize {
        self.len
    }

    pub(in crate::scan::parallel) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The lowest class that holds work.
    pub(in crate::scan::parallel) fn best_class(&self) -> Option<usize> {
        self.tiers.iter().position(|tier| !tier.is_empty())
    }

    pub(in crate::scan::parallel) fn push(&mut self, class: usize, arrival: Arrival) {
        inspected(1);
        let depth = Depth(arrival.1.depth());
        let root = Root(arrival.1.root_index);
        self.tiers[class.min(NORMAL)]
            .entry(depth)
            .or_default()
            .entry(root)
            .or_default()
            .push_back(arrival);
        self.len += 1;
    }

    /// Removes the earliest item of the lowest class and shallowest depth.
    ///
    /// Within that depth the first root after `last_root` is served, wrapping
    /// around to the lowest root index, so roots of one lane take turns.
    pub(in crate::scan::parallel) fn pop(&mut self, last_root: Option<u32>) -> Option<Frame> {
        let class = self.best_class()?;
        let mut shallowest = self.tiers[class].first_entry()?;
        let roots = shallowest.get_mut();
        let after = last_root.and_then(|last| {
            roots
                .range((Bound::Excluded(Root(last)), Bound::Unbounded))
                .next()
                .map(|(root, _)| *root)
        });
        let root = after.or_else(|| roots.keys().next().copied())?;
        let queue = roots.get_mut(&root)?;
        let (_, frame) = queue.pop_front()?;
        if queue.is_empty() {
            roots.remove(&root);
        }
        if roots.is_empty() {
            shallowest.remove();
        }
        self.len -= 1;
        inspected(1);
        Some(frame)
    }

    /// Removes every item of `root` from `classes`, visiting only those items.
    pub(in crate::scan::parallel) fn take_root(&mut self, classes: &[usize], root: u32) -> Vec<Arrival> {
        let mut taken = Vec::new();
        for &class in classes {
            self.tiers[class.min(NORMAL)].retain(|_, roots| {
                if let Some(queue) = roots.remove(&Root(root)) {
                    taken.extend(queue);
                }
                !roots.is_empty()
            });
        }
        self.len -= taken.len();
        inspected(taken.len());
        taken
    }

    /// Returns items to `class`, keeping arrival order with items already there.
    pub(in crate::scan::parallel) fn restore(&mut self, class: usize, mut arrivals: Vec<Arrival>) {
        arrivals.sort_by_key(|(order, _)| *order);
        self.len += arrivals.len();
        let mut groups: HashMap<(usize, u32), Vec<Arrival>> = HashMap::new();
        for arrival in arrivals {
            let key = (arrival.1.depth(), arrival.1.root_index);
            groups.entry(key).or_default().push(arrival);
        }
        let tier = &mut self.tiers[class.min(NORMAL)];
        for ((depth, root), group) in groups {
            let queue = tier
                .entry(Depth(depth))
                .or_default()
                .entry(Root(root))
                .or_default();
            merge_in_order(queue, group);
        }
    }
}

/// Merges items sorted by arrival into a queue sorted by arrival, in one pass.
fn merge_in_order(queue: &mut VecDeque<Arrival>, incoming: Vec<Arrival>) {
    let first = incoming.first().map(|(order, _)| *order);
    if queue
        .back()
        .is_none_or(|(order, _)| first.is_none_or(|first| *order < first))
    {
        queue.extend(incoming);
        return;
    }
    let mut existing = std::mem::take(queue).into_iter().peekable();
    let mut incoming = incoming.into_iter().peekable();
    loop {
        let from_existing = match (existing.peek(), incoming.peek()) {
            (Some(left), Some(right)) => left.0 < right.0,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let next = if from_existing {
            existing.next()
        } else {
            incoming.next()
        };
        queue.extend(next);
    }
}

/// The overflow stack of one worker, kept per class in arrival order.
#[derive(Default)]
pub(in crate::scan::parallel) struct Stack {
    classes: [VecDeque<Arrival>; CLASSES],
}

impl Stack {
    pub(in crate::scan::parallel) fn best_class(&self) -> Option<usize> {
        self.classes.iter().position(|items| !items.is_empty())
    }

    pub(in crate::scan::parallel) fn push(&mut self, class: usize, arrival: Arrival) {
        inspected(1);
        self.classes[class.min(NORMAL)].push_back(arrival);
    }

    /// The newest item of the lowest class, continuing the owner's chain.
    pub(in crate::scan::parallel) fn pop_top(&mut self) -> Option<Frame> {
        let class = self.best_class()?;
        inspected(1);
        self.classes[class].pop_back().map(|(_, frame)| frame)
    }

    /// Class and depth of the oldest item of the lowest class, for stealing.
    pub(in crate::scan::parallel) fn bottom(&self) -> Option<(usize, usize)> {
        let class = self.best_class()?;
        inspected(1);
        self.classes[class]
            .front()
            .map(|(_, frame)| (class, frame.depth()))
    }

    /// Removes the oldest item of the lowest class; it heads the largest subtree.
    pub(in crate::scan::parallel) fn pop_bottom(&mut self) -> Option<Frame> {
        let class = self.best_class()?;
        self.classes[class].pop_front().map(|(_, frame)| frame)
    }

    /// Removes every item of `root` from `classes` in one pass over them.
    ///
    /// A stack is normally one depth-first chain of a single root, so the pass
    /// mostly visits items of that root.
    pub(in crate::scan::parallel) fn take_root(&mut self, classes: &[usize], root: u32) -> Vec<Arrival> {
        let mut taken = Vec::new();
        for &class in classes {
            let items = &mut self.classes[class.min(NORMAL)];
            inspected(items.len());
            let mut kept = VecDeque::with_capacity(items.len());
            for item in items.drain(..) {
                if item.1.root_index == root {
                    taken.push(item);
                } else {
                    kept.push_back(item);
                }
            }
            *items = kept;
        }
        taken
    }

    /// Returns items to `class`, keeping arrival order with items already there.
    pub(in crate::scan::parallel) fn restore(&mut self, class: usize, mut arrivals: Vec<Arrival>) {
        arrivals.sort_by_key(|(order, _)| *order);
        merge_in_order(&mut self.classes[class.min(NORMAL)], arrivals);
    }

    /// Removes every item, lowest class first, for draining or leaving a lane.
    pub(in crate::scan::parallel) fn drain(&mut self) -> Vec<(usize, Frame)> {
        let mut drained = Vec::new();
        for (class, items) in self.classes.iter_mut().enumerate() {
            drained.extend(items.drain(..).map(|(_, frame)| (class, frame)));
        }
        drained
    }
}
