//! Priority areas over outstanding directory work and optional peak accounting.
use super::{lanes::NORMAL, *};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PriorityRegion {
    pub root_index: u32,
    pub path: Vec<Vec<u8>>,
}

pub(super) enum PriorityDecision {
    Accepted { remaining: usize },
    NoEffect { same_region: bool, remaining: usize },
}

#[derive(Default)]
struct OutstandingNode {
    count: usize,
    children: HashMap<Vec<u8>, usize>,
}

/// Counts outstanding directory work in a flat arena, sharing raw path prefixes.
/// Nodes are pruned iteratively when their last outstanding task completes.
pub(super) struct OutstandingPaths {
    roots: Vec<usize>,
    nodes: Vec<OutstandingNode>,
    free: Vec<usize>,
}

impl OutstandingPaths {
    pub(super) fn new(root_count: usize) -> Self {
        Self {
            roots: (0..root_count).collect(),
            nodes: (0..root_count)
                .map(|_| OutstandingNode::default())
                .collect(),
            free: Vec::new(),
        }
    }

    pub(super) fn register(&mut self, root_index: u32, path: &[Vec<u8>]) -> bool {
        let Some(&root) = self.roots.get(root_index as usize) else {
            return false;
        };
        let mut chain = vec![root];
        let mut node = root;
        for component in path {
            let Some(current) = self.nodes.get(node) else {
                return false;
            };
            let child = if let Some(&child) = current.children.get(component) {
                Some(child)
            } else {
                self.add_child(node, component)
            };
            let Some(child) = child else {
                return false;
            };
            node = child;
            chain.push(node);
        }
        for node in chain {
            self.nodes[node].count += 1;
        }
        true
    }

    pub(super) fn complete(&mut self, root_index: u32, path: &[Vec<u8>]) -> bool {
        let Some(&root) = self.roots.get(root_index as usize) else {
            return false;
        };
        let mut chain = vec![root];
        let mut node = root;
        for component in path {
            let Some(&child) = self
                .nodes
                .get(node)
                .and_then(|node| node.children.get(component))
            else {
                return false;
            };
            node = child;
            chain.push(node);
        }
        if chain.iter().any(|&index| self.nodes[index].count == 0) {
            return false;
        }
        for &index in &chain {
            self.nodes[index].count -= 1;
        }
        for depth in (1..chain.len()).rev() {
            let child = chain[depth];
            if self.nodes[child].count != 0 || !self.nodes[child].children.is_empty() {
                break;
            }
            let parent = chain[depth - 1];
            self.nodes[parent].children.remove(&path[depth - 1]);
            self.nodes[child] = OutstandingNode::default();
            self.free.push(child);
        }
        true
    }

    pub(super) fn count(&self, root_index: u32, path: &[Vec<u8>]) -> usize {
        let Some(&root) = self.roots.get(root_index as usize) else {
            return 0;
        };
        let mut node = root;
        for component in path {
            let Some(&child) = self.nodes[node].children.get(component) else {
                return 0;
            };
            node = child;
        }
        self.nodes[node].count
    }

    fn add_child(&mut self, parent: usize, component: &[u8]) -> Option<usize> {
        if self.nodes.get(parent)?.children.contains_key(component) {
            return None;
        }
        let index = if let Some(index) = self.free.pop() {
            *self.nodes.get_mut(index)? = OutstandingNode::default();
            index
        } else {
            let index = self.nodes.len();
            self.nodes.push(OutstandingNode::default());
            index
        };
        self.nodes
            .get_mut(parent)?
            .children
            .insert(component.to_vec(), index);
        Some(index)
    }
}

impl Queue {
    /// Changes only the next scheduling order. The caller resolves scope authority.
    ///
    /// Accepting a new or replacement area moves the held items of the old and
    /// new area roots between priority classes once; claims never scan items.
    pub(super) fn prioritize(&mut self, region: PriorityRegion) -> PriorityDecision {
        let Some(outstanding) = self.outstanding.as_ref() else {
            return PriorityDecision::NoEffect {
                same_region: false,
                remaining: 0,
            };
        };
        let remaining = outstanding.count(region.root_index, &region.path);
        if self.priority.as_ref() == Some(&region) {
            return PriorityDecision::NoEffect {
                same_region: true,
                remaining,
            };
        }
        if remaining == 0 {
            return PriorityDecision::NoEffect {
                same_region: false,
                remaining,
            };
        }
        let previous = self.priority.replace(region).map(|old| old.root_index);
        self.reclassify(previous);
        PriorityDecision::Accepted { remaining }
    }

    pub(super) fn complete_path(&mut self, root_index: u32, path: &[Vec<u8>]) -> bool {
        let Some(outstanding) = self.outstanding.as_mut() else {
            return true;
        };
        if !outstanding.complete(root_index, path) {
            return false;
        }
        let finished = self
            .priority
            .as_ref()
            .is_some_and(|region| outstanding.count(region.root_index, &region.path) == 0);
        if finished {
            // Only ancestors of the finished area can still be marked.
            let previous = self.priority.take().map(|old| old.root_index);
            self.reclassify(previous);
        }
        true
    }

    /// The priority class of `frame` under the current area.
    pub(super) fn priority_rank(&self, frame: &Frame) -> usize {
        rank_in(self.priority.as_ref(), frame)
    }
}

/// Class 0 lies inside the area, class 1 is an ancestor of it and every other
/// item, or every item without an area, is in the normal class.
pub(super) fn rank_in(region: Option<&PriorityRegion>, frame: &Frame) -> usize {
    let Some(region) = region else {
        return NORMAL;
    };
    if frame.root_index != region.root_index {
        NORMAL
    } else if frame.path.starts_with(&region.path) {
        0
    } else if region.path.starts_with(&frame.path) {
        1
    } else {
        NORMAL
    }
}

pub(super) fn record_ready_peak(
    shared: &Shared,
    diagnostics: Option<&crate::scan_job::ScanDiagnostics>,
) {
    if let Some(observer) = diagnostics
        && let Ok(queue) = shared.queue.lock()
        && let Some(peak) = queue.lanes.peak()
    {
        observer.record_ready_peak(peak);
    }
}

/// Device groups of the scope roots and the worker limit of each group's lane.
pub(crate) struct DevicePlan {
    pub groups: Vec<usize>,
    pub limit: usize,
}
impl DevicePlan {
    pub(super) fn valid(&self, count: usize) -> bool {
        count != 0
            && u32::try_from(count).is_ok()
            && self.groups.len() == count
            && self.limit != 0
            && self.groups.iter().all(|&group| group < count)
    }
}
