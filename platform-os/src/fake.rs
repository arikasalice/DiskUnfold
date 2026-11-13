use du_core_contracts::ports::{
    Clock, ContentReadPort, Entry, EntryKind, EnumeratePort, PortError, PortErrorKind, Random,
};
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Directory,
    File(Vec<u8>),
    Link(Vec<u8>),
    Placeholder,
    Volume,
}
impl Node {
    fn kind(&self) -> EntryKind {
        match self {
            Self::Directory => EntryKind::Directory,
            Self::File(_) => EntryKind::File,
            Self::Link(_) => EntryKind::Link,
            Self::Placeholder => EntryKind::Placeholder,
            Self::Volume => EntryKind::Volume,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Enumerate,
    Read,
}
#[derive(Clone, Debug)]
pub enum Fault {
    Error(PortError),
    Delay(u64),
}
#[derive(Clone, Debug)]
pub struct FaultPlan {
    pub path: Vec<u8>,
    pub operation: Operation,
    pub fault: Fault,
    pub remaining: u32,
}
struct State {
    nodes: BTreeMap<Vec<u8>, Node>,
    clock: u64,
    random: u64,
    faults: Vec<FaultPlan>,
}
pub struct FakeWorld {
    state: Mutex<State>,
}
fn error(kind: PortErrorKind, reason: &str) -> PortError {
    PortError {
        kind,
        reason_key: reason.to_owned(),
        os_code: None,
    }
}
impl FakeWorld {
    /// Paths are absolute slash-separated byte strings. Parent nodes must be directories or volumes.
    pub fn new(nodes: Vec<(Vec<u8>, Node)>, seed: u64) -> Result<Self, PortError> {
        let mut tree = BTreeMap::new();
        for (path, node) in nodes {
            if !valid_path(&path) || tree.insert(path, node).is_some() {
                return Err(error(PortErrorKind::Corrupt, "os.invalid_tree"));
            }
        }
        if !matches!(
            tree.get(b"/".as_slice()),
            Some(Node::Directory | Node::Volume)
        ) {
            return Err(error(PortErrorKind::Corrupt, "os.invalid_tree"));
        }
        for path in tree.keys().filter(|p| p.as_slice() != b"/") {
            let Some(pos) = path.iter().rposition(|b| *b == b'/') else {
                return Err(error(PortErrorKind::Corrupt, "os.invalid_tree"));
            };
            let parent = if pos == 0 {
                b"/".as_slice()
            } else {
                &path[..pos]
            };
            if !matches!(tree.get(parent), Some(Node::Directory | Node::Volume)) {
                return Err(error(PortErrorKind::Corrupt, "os.invalid_tree"));
            }
        }
        Ok(Self {
            state: Mutex::new(State {
                nodes: tree,
                clock: 0,
                random: seed,
                faults: Vec::new(),
            }),
        })
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    pub fn add_fault(&self, plan: FaultPlan) {
        self.state().faults.push(plan);
    }
    pub fn advance(&self, millis: u64) {
        let mut s = self.state();
        s.clock = s.clock.saturating_add(millis);
    }
    fn apply_fault(state: &mut State, operation: Operation, path: &[u8]) -> Result<(), PortError> {
        if let Some(plan) = state
            .faults
            .iter_mut()
            .find(|f| f.operation == operation && f.path == path && f.remaining > 0)
        {
            plan.remaining -= 1;
            match &plan.fault {
                Fault::Error(e) => return Err(e.clone()),
                Fault::Delay(ms) => state.clock = state.clock.saturating_add(*ms),
            }
        }
        Ok(())
    }
}
fn valid_path(p: &[u8]) -> bool {
    !p.contains(&0)
        && (p == b"/"
            || (p.starts_with(b"/")
                && !p.ends_with(b"/")
                && !p
                    .split(|b| *b == b'/')
                    .skip(1)
                    .any(|c| c.is_empty() || c == b"." || c == b"..")))
}
impl EnumeratePort for FakeWorld {
    fn enumerate(&self, path: &[u8]) -> Result<Vec<Entry>, PortError> {
        let mut state = self.state();
        Self::apply_fault(&mut state, Operation::Enumerate, path)?;
        match state.nodes.get(path) {
            Some(Node::Directory | Node::Volume) => {}
            Some(_) => return Err(error(PortErrorKind::Unsupported, "os.not_directory")),
            None => return Err(error(PortErrorKind::NotFound, "os.not_found")),
        }
        let mut prefix = path.to_vec();
        if path != b"/" {
            prefix.push(b'/');
        }
        Ok(state
            .nodes
            .iter()
            .filter(|(p, _)| {
                p.starts_with(&prefix)
                    && p.len() > prefix.len()
                    && !p[prefix.len()..].contains(&b'/')
            })
            .map(|(path, node)| Entry {
                path: path.clone(),
                kind: node.kind(),
            })
            .collect())
    }
}
impl ContentReadPort for FakeWorld {
    fn read(&self, path: &[u8], limit: usize) -> Result<Vec<u8>, PortError> {
        let mut state = self.state();
        Self::apply_fault(&mut state, Operation::Read, path)?;
        match state.nodes.get(path) {
            Some(Node::File(bytes)) => Ok(bytes[..bytes.len().min(limit)].to_vec()),
            Some(_) => Err(error(PortErrorKind::Unsupported, "os.not_file")),
            None => Err(error(PortErrorKind::NotFound, "os.not_found")),
        }
    }
}
impl Clock for FakeWorld {
    fn monotonic_millis(&self) -> u64 {
        self.state().clock
    }
}
impl Random for FakeWorld {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        let mut state = self.state();
        for byte in bytes {
            state.random = state
                .random
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *byte = (state.random >> 56) as u8;
        }
        Ok(())
    }
}
