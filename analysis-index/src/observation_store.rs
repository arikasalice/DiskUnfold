//! Observation storage for one result build, with shared path nodes.
//!
//! Every observed item is one node that refers to its parent node and to its
//! own name. A name is stored once, so the memory held for an item does not
//! grow with its depth. Lookups walk the path from its root through a hash
//! index of `(parent, name)`; a short list of recently used directories lets
//! consecutive entries of one directory skip that walk.
use crate::StoredObservation;
use std::hash::{BuildHasher, RandomState};

const CHUNK_BITS: u32 = 12;
const CHUNK_MASK: u32 = (1 << CHUNK_BITS) - 1;
const NO_NODE: u32 = u32::MAX;
const RECENT_DIRECTORIES: usize = 8;
const INITIAL_SLOTS: usize = 1024;
/// Node numbers stay below `NO_NODE` and `node + 1` fits an index slot.
const MAX_NODES: u32 = u32::MAX - 1;

/// Why an observation was not stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InsertError {
    /// The root and path already hold an observation.
    Duplicate,
    /// The node limit of one build is reached.
    Capacity,
}

struct Node {
    /// Parent node, or `NO_NODE` for a root.
    parent: u32,
    name_len: u32,
    name_start: usize,
    /// `None` marks an ancestor that was named by a path before it was observed.
    observation: Option<StoredObservation>,
}

/// Nodes, names and the child index of one build.
///
/// Nodes are numbered in creation order and never removed. A root is the
/// node of an empty path; every other node has a parent and a non-empty name.
pub(crate) struct ObservationStore {
    /// Fixed-size chunks, so growth never copies or over-allocates all nodes.
    chunks: Vec<Vec<Node>>,
    nodes: u32,
    observed: u32,
    names: Vec<u8>,
    /// Open-addressed `(parent, name)` index; a slot holds `node + 1` or zero.
    slots: Vec<u32>,
    /// Number of nodes entered in `slots`.
    indexed: usize,
    /// Root node by root index, or `NO_NODE`.
    roots: Vec<u32>,
    hasher: RandomState,
    recent: [u32; RECENT_DIRECTORIES],
    next_recent: usize,
    /// The node most recently stored or located.
    last: u32,
    limit: u32,
    /// Observed file nodes outside the scan boundary, ascending (F03 FR5.HD1).
    boundary: Vec<u32>,
}

impl Default for ObservationStore {
    fn default() -> Self {
        Self::with_limit(MAX_NODES)
    }
}

impl ObservationStore {
    pub(crate) fn with_limit(limit: u32) -> Self {
        Self {
            chunks: Vec::new(),
            nodes: 0,
            observed: 0,
            names: Vec::new(),
            slots: Vec::new(),
            indexed: 0,
            roots: Vec::new(),
            hasher: RandomState::new(),
            recent: [NO_NODE; RECENT_DIRECTORIES],
            next_recent: 0,
            last: NO_NODE,
            limit: limit.min(MAX_NODES),
            boundary: Vec::new(),
        }
    }

    /// Marks an observed node as a file outside the scan boundary.
    pub(crate) fn mark_boundary(&mut self, node: u32) {
        if let Err(index) = self.boundary.binary_search(&node) {
            self.boundary.insert(index, node);
        }
    }

    /// Whether a node is a file outside the scan boundary.
    pub(crate) fn is_boundary(&self, node: u32) -> bool {
        self.boundary.binary_search(&node).is_ok()
    }

    /// Boundary file nodes in ascending node order.
    pub(crate) fn boundary_nodes(&self) -> &[u32] {
        &self.boundary
    }

    /// Number of nodes, including ancestors that are not observed yet.
    pub(crate) fn nodes(&self) -> u32 {
        self.nodes
    }

    /// Whether no observation was stored.
    pub(crate) fn is_empty(&self) -> bool {
        self.observed == 0
    }

    /// Whether a path named an ancestor that has no observation.
    pub(crate) fn has_unobserved(&self) -> bool {
        self.observed != self.nodes
    }

    pub(crate) fn root_node(&self, root: u32) -> Option<u32> {
        self.roots
            .get(root as usize)
            .copied()
            .filter(|node| *node != NO_NODE)
    }

    pub(crate) fn parent(&self, node: u32) -> Option<u32> {
        Some(self.node(node).parent).filter(|parent| *parent != NO_NODE)
    }

    pub(crate) fn name(&self, node: u32) -> &[u8] {
        let node = self.node(node);
        &self.names[node.name_start..node.name_start + node.name_len as usize]
    }

    pub(crate) fn observation(&self, node: u32) -> Option<&StoredObservation> {
        self.node(node).observation.as_ref()
    }

    /// Stores the observation of `path` below `root`, creating missing ancestors unobserved.
    ///
    /// The caller validates the root index and rejects empty components.
    pub(crate) fn insert(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
        observation: StoredObservation,
    ) -> Result<(), InsertError> {
        let node = match path.split_last() {
            Some((name, directory)) => {
                let parent = self.ensure_directory(root, directory)?;
                match self.child(parent, name) {
                    Some(node) => node,
                    None => self.push(parent, name)?,
                }
            }
            None => self.ensure_root(root)?,
        };
        let slot = &mut self.node_mut(node).observation;
        if slot.is_some() {
            return Err(InsertError::Duplicate);
        }
        *slot = Some(observation);
        self.observed += 1;
        self.last = node;
        Ok(())
    }

    /// Finds the node of an exact path and remembers it for the next lookup.
    pub(crate) fn locate(&mut self, root: u32, path: &[Vec<u8>]) -> Option<u32> {
        if self.last != NO_NODE && self.is_path_of(self.last, root, path) {
            return Some(self.last);
        }
        let node = match path.split_last() {
            Some((name, directory)) => {
                let parent = match self.recent_directory(root, directory) {
                    Some(parent) => parent,
                    None => self.find(root, directory)?,
                };
                self.child(parent, name)?
            }
            None => self.root_node(root)?,
        };
        self.last = node;
        Some(node)
    }

    /// The observation stored for an exact path, if that path was observed.
    pub(crate) fn get_mut(
        &mut self,
        root: u32,
        path: &[Vec<u8>],
    ) -> Option<&mut StoredObservation> {
        let node = self.locate(root, path)?;
        self.node_mut(node).observation.as_mut()
    }

    /// Finds the node of an exact path without using or updating the lookup memory.
    pub(crate) fn find(&self, root: u32, path: &[Vec<u8>]) -> Option<u32> {
        let mut node = self.root_node(root)?;
        for name in path {
            node = self.child(node, name)?;
        }
        Some(node)
    }

    /// Finds one direct child without reconstructing its full path.
    pub(crate) fn child_node(&self, parent: u32, name: &[u8]) -> Option<u32> {
        self.child(parent, name)
    }

    /// The deepest existing node on `path` and whether it is the whole path.
    pub(crate) fn deepest(&self, root: u32, path: &[Vec<u8>]) -> Option<(u32, bool)> {
        let mut node = self.root_node(root)?;
        for name in path {
            match self.child(node, name) {
                Some(child) => node = child,
                None => return Some((node, false)),
            }
        }
        Some((node, true))
    }

    fn node(&self, node: u32) -> &Node {
        &self.chunks[(node >> CHUNK_BITS) as usize][(node & CHUNK_MASK) as usize]
    }

    fn node_mut(&mut self, node: u32) -> &mut Node {
        &mut self.chunks[(node >> CHUNK_BITS) as usize][(node & CHUNK_MASK) as usize]
    }

    /// Whether `node` is the node of `path` below `root`, compared from the last name upwards.
    fn is_path_of(&self, mut node: u32, root: u32, path: &[Vec<u8>]) -> bool {
        for name in path.iter().rev() {
            let parent = self.node(node).parent;
            if parent == NO_NODE || self.name(node) != name.as_slice() {
                return false;
            }
            node = parent;
        }
        self.root_node(root) == Some(node)
    }

    fn recent_directory(&self, root: u32, path: &[Vec<u8>]) -> Option<u32> {
        (1..=RECENT_DIRECTORIES)
            .map(|age| {
                self.recent[(self.next_recent + RECENT_DIRECTORIES - age) % RECENT_DIRECTORIES]
            })
            .find(|node| *node != NO_NODE && self.is_path_of(*node, root, path))
    }

    fn remember_directory(&mut self, node: u32) {
        self.recent[self.next_recent] = node;
        self.next_recent = (self.next_recent + 1) % RECENT_DIRECTORIES;
    }

    fn ensure_root(&mut self, root: u32) -> Result<u32, InsertError> {
        if let Some(node) = self.root_node(root) {
            return Ok(node);
        }
        let index = root as usize;
        if self.roots.len() <= index {
            self.roots.resize(index + 1, NO_NODE);
        }
        let node = self.push_node(NO_NODE, &[])?;
        self.roots[index] = node;
        Ok(node)
    }

    fn ensure_directory(&mut self, root: u32, path: &[Vec<u8>]) -> Result<u32, InsertError> {
        if path.is_empty() {
            return self.ensure_root(root);
        }
        if let Some(node) = self.recent_directory(root, path) {
            return Ok(node);
        }
        let mut node = self.ensure_root(root)?;
        for name in path {
            node = match self.child(node, name) {
                Some(child) => child,
                None => self.push(node, name)?,
            };
        }
        self.remember_directory(node);
        Ok(node)
    }

    fn hash(&self, parent: u32, name: &[u8]) -> usize {
        // Truncation keeps the low bits, which the power-of-two mask selects.
        self.hasher.hash_one((parent, name)) as usize
    }

    fn child(&self, parent: u32, name: &[u8]) -> Option<u32> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut index = self.hash(parent, name) & mask;
        // The index is at most three quarters full, so an empty slot ends every probe.
        loop {
            let node = self.slots[index].checked_sub(1)?;
            if self.node(node).parent == parent && self.name(node) == name {
                return Some(node);
            }
            index = (index + 1) & mask;
        }
    }

    /// Adds a named child that the index does not contain yet.
    fn push(&mut self, parent: u32, name: &[u8]) -> Result<u32, InsertError> {
        let node = self.push_node(parent, name)?;
        self.indexed += 1;
        if self.indexed.saturating_mul(4) > self.slots.len().saturating_mul(3) {
            self.grow_index();
        } else {
            let slot = self.free_slot(&self.slots, parent, name);
            self.slots[slot] = node + 1;
        }
        Ok(node)
    }

    fn push_node(&mut self, parent: u32, name: &[u8]) -> Result<u32, InsertError> {
        let name_len = u32::try_from(name.len()).map_err(|_| InsertError::Capacity)?;
        if self.nodes >= self.limit {
            return Err(InsertError::Capacity);
        }
        let node = self.nodes;
        if node & CHUNK_MASK == 0 {
            self.chunks.push(Vec::with_capacity(1 << CHUNK_BITS));
        }
        let name_start = self.names.len();
        self.names.extend_from_slice(name);
        self.chunks[(node >> CHUNK_BITS) as usize].push(Node {
            parent,
            name_len,
            name_start,
            observation: None,
        });
        self.nodes += 1;
        Ok(node)
    }

    fn free_slot(&self, slots: &[u32], parent: u32, name: &[u8]) -> usize {
        let mask = slots.len() - 1;
        let mut index = self.hash(parent, name) & mask;
        while slots[index] != 0 {
            index = (index + 1) & mask;
        }
        index
    }

    /// Doubles the index and re-enters every named node, including the newest.
    fn grow_index(&mut self) {
        let mut slots = vec![0_u32; (self.slots.len() * 2).max(INITIAL_SLOTS)];
        for node in 0..self.nodes {
            let parent = self.node(node).parent;
            if parent != NO_NODE {
                let slot = self.free_slot(&slots, parent, self.name(node));
                slots[slot] = node + 1;
            }
        }
        self.slots = slots;
    }
}

#[cfg(test)]
mod tests;
