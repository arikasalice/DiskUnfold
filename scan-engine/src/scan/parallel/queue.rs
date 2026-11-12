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
    children: HashMap<Vec<u8>, OutstandingNode>,
}

/// Counts outstanding directory work while sharing raw path component prefixes.
/// Nodes are pruned when their last outstanding directory task completes.
pub(super) struct OutstandingPaths {
    roots: Vec<OutstandingNode>,
}

impl OutstandingPaths {
    pub(super) fn new(root_count: usize) -> Self {
        Self {
            roots: (0..root_count)
                .map(|_| OutstandingNode::default())
                .collect(),
        }
    }

    pub(super) fn register(&mut self, root_index: u32, path: &[Vec<u8>]) -> bool {
        let Some(mut node) = self.roots.get_mut(root_index as usize) else {
            return false;
        };
        node.count += 1;
        for component in path {
            node = node.children.entry(component.clone()).or_default();
            node.count += 1;
        }
        true
    }

    pub(super) fn complete(&mut self, root_index: u32, path: &[Vec<u8>]) -> bool {
        let Some(root) = self.roots.get_mut(root_index as usize) else {
            return false;
        };
        release_path(root, path).is_ok()
    }

    pub(super) fn count(&self, root_index: u32, path: &[Vec<u8>]) -> usize {
        let Some(mut node) = self.roots.get(root_index as usize) else {
            return 0;
        };
        for component in path {
            let Some(child) = node.children.get(component) else {
                return 0;
            };
            node = child;
        }
        node.count
    }
}

fn release_path(node: &mut OutstandingNode, path: &[Vec<u8>]) -> Result<bool, ()> {
    if node.count == 0 {
        return Err(());
    }
    node.count -= 1;
    if let Some((component, rest)) = path.split_first() {
        let prune_child = match node
            .children
            .get_mut(component)
            .ok_or(())
            .and_then(|child| release_path(child, rest))
        {
            Ok(prune_child) => prune_child,
            Err(()) => {
                node.count += 1;
                return Err(());
            }
        };
        if prune_child {
            node.children.remove(component);
        }
    }
    Ok(node.count == 0 && node.children.is_empty())
}

impl Queue {
    /// Changes only the next scheduling order. The caller resolves scope authority.
    pub(super) fn prioritize(&mut self, region: PriorityRegion) -> PriorityDecision {
        let remaining = self.outstanding.count(region.root_index, &region.path);
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
        if !self.outstanding.complete(root_index, path) {
            return false;
        }
        if self
            .priority
            .as_ref()
            .is_some_and(|region| self.outstanding.count(region.root_index, &region.path) == 0)
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
