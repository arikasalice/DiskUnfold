//! Shared-ready deque scheduling and optional peak accounting.
use super::*;
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
        self.priority = Some(region);
        PriorityDecision::Accepted { remaining }
    }

    pub(super) fn complete_path(&mut self, root_index: u32, path: &[Vec<u8>]) -> bool {
        let Some(outstanding) = self.outstanding.as_mut() else {
            return true;
        };
        if !outstanding.complete(root_index, path) {
            return false;
        }
        if self
            .priority
            .as_ref()
            .is_some_and(|region| outstanding.count(region.root_index, &region.path) == 0)
        {
            self.priority = None;
        }
        true
    }

    pub(super) fn priority_rank(&self, frame: &Frame) -> u8 {
        let Some(region) = &self.priority else {
            return 0;
        };
        if frame.root_index != region.root_index {
            return 2;
        }
        if frame.path.starts_with(&region.path) {
            0
        } else if region.path.starts_with(&frame.path) {
            1
        } else {
            2
        }
    }

    pub(super) fn best_ready_rank(&self) -> Option<u8> {
        if !self.started {
            return None;
        }
        self.ready
            .iter()
            .filter(|frame| {
                self.active_groups[self.root_groups[frame.root_index as usize]] < self.device_limit
            })
            .map(|frame| self.priority_rank(frame))
            .min()
    }

    pub(super) fn push_ready(&mut self, frame: Frame) {
        self.ready.push_back(frame);
        if let Some(peak) = &mut self.ready_high_water {
            *peak = (*peak).max(self.ready.len());
        }
    }

    pub(super) fn pop_fair(&mut self) -> Option<(Frame, usize)> {
        let last = self.last_root;
        let index = self
            .ready
            .iter()
            .enumerate()
            .filter(|(_, frame)| {
                self.active_groups[self.root_groups[frame.root_index as usize]] < self.device_limit
            })
            .min_by_key(|(index, frame)| {
                (
                    self.priority_rank(frame),
                    last.is_some_and(|value| frame.root_index <= value),
                    frame.root_index,
                    *index,
                )
            })
            .map(|(index, _)| index)?;
        let frame = self.ready.remove(index)?;
        self.last_root = Some(frame.root_index);
        let group = self.root_groups[frame.root_index as usize];
        self.active_groups[group] += 1;
        Some((frame, group))
    }
}

pub(super) fn record_ready_peak(
    shared: &Shared,
    diagnostics: Option<&crate::scan_job::ScanDiagnostics>,
) {
    if let Some(observer) = diagnostics
        && let Ok(queue) = shared.queue.lock()
        && let Some(peak) = queue.ready_high_water
    {
        observer.record_ready_peak(peak);
    }
}

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
