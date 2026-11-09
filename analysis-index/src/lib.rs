//! In-memory result publication and conservative per-subtree space aggregation.
mod space;
use du_core_contracts::result::{
    ResultCompleteness, ResultItem, ResultVersionRead, SpaceAggregate,
};
use du_core_contracts::{Id, SizeValue};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildError {
    Busy,
    InvalidTree,
    DuplicateVersion,
    CapacityExceeded,
    Unavailable,
}

/// Raw component names relative to the root; the root path is empty.
#[derive(Clone, Debug)]
pub struct Observation {
    pub path: Vec<Vec<u8>>,
    pub size: Option<SizeValue>,
}

#[derive(Debug)]
pub struct ResultVersion {
    version_id: Id,
    scope_version_id: Id,
    completeness: ResultCompleteness,
    items: Vec<ResultItem>,
    aggregates: Vec<SpaceAggregate>,
}
impl ResultVersion {
    /// Returns the aggregate for an item in this pinned version, or `None` for an invalid number.
    pub fn space(&self, item_no: u32) -> Option<&SpaceAggregate> {
        self.aggregates.get(item_no as usize)
    }
}
impl ResultVersionRead for ResultVersion {
    fn version_id(&self) -> &Id {
        &self.version_id
    }
    fn scope_version_id(&self) -> &Id {
        &self.scope_version_id
    }
    fn completeness(&self) -> ResultCompleteness {
        self.completeness
    }
    fn items(&self) -> &[ResultItem] {
        &self.items
    }
}

#[derive(Default)]
struct State {
    current: Option<Arc<ResultVersion>>,
    active: bool,
    used_versions: HashSet<Id>,
}
/// A single publication point. Readers retain their snapshot after releasing the lock.
#[derive(Clone, Default)]
pub struct IngestSink {
    state: Arc<Mutex<State>>,
}
impl IngestSink {
    pub fn pin(&self) -> Result<Option<Arc<ResultVersion>>, BuildError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .current
            .clone())
    }
    pub fn open(&self, scope_version_id: Id) -> Result<BuildHandle, BuildError> {
        let mut state = self.state.lock().map_err(|_| BuildError::Unavailable)?;
        if state.active {
            return Err(BuildError::Busy);
        }
        state.active = true;
        Ok(BuildHandle {
            state: Arc::clone(&self.state),
            baseline: state.current.clone(),
            scope_version_id,
            observations: BTreeMap::new(),
            finished: false,
        })
    }
}

/// Dropping or aborting an unfinished build withdraws its partial publications.
pub struct BuildHandle {
    state: Arc<Mutex<State>>,
    baseline: Option<Arc<ResultVersion>>,
    scope_version_id: Id,
    observations: BTreeMap<Vec<Vec<u8>>, Option<SizeValue>>,
    finished: bool,
}
impl BuildHandle {
    pub fn add(&mut self, observation: Observation) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        if observation.path.iter().any(Vec::is_empty)
            || self.observations.contains_key(&observation.path)
        {
            self.abort();
            return Err(BuildError::InvalidTree);
        }
        self.observations.insert(observation.path, observation.size);
        Ok(())
    }
    pub fn publish_partial(&mut self, version_id: Id) -> Result<Arc<ResultVersion>, BuildError> {
        self.publish(version_id, ResultCompleteness::Partial, false)
    }
    pub fn seal(
        mut self,
        version_id: Id,
        completeness: ResultCompleteness,
    ) -> Result<Arc<ResultVersion>, BuildError> {
        self.publish(version_id, completeness, true)
    }
    fn publish(
        &mut self,
        version_id: Id,
        completeness: ResultCompleteness,
        finish: bool,
    ) -> Result<Arc<ResultVersion>, BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        let items = match build_items(&self.observations) {
            Ok(items) => items,
            Err(error) => {
                self.abort();
                return Err(error);
            }
        };
        let aggregates = space::aggregate(&items, completeness);
        let result = Arc::new(ResultVersion {
            version_id,
            scope_version_id: self.scope_version_id.clone(),
            completeness,
            items,
            aggregates,
        });
        let mut state = self.state.lock().map_err(|_| BuildError::Unavailable)?;
        if !state.used_versions.insert(result.version_id.clone()) {
            drop(state);
            self.abort();
            return Err(BuildError::DuplicateVersion);
        }
        let previous = state.current.replace(Arc::clone(&result));
        if finish {
            state.active = false;
            self.finished = true;
        }
        drop(state);
        drop(previous);
        Ok(result)
    }
    pub fn abort(&mut self) {
        if !self.finished {
            let withdrawn = self.state.lock().ok().and_then(|mut state| {
                state.active = false;
                std::mem::replace(&mut state.current, self.baseline.clone())
            });
            drop(withdrawn);
            self.finished = true;
        }
    }
}
impl Drop for BuildHandle {
    fn drop(&mut self) {
        self.abort();
    }
}

fn build_items(
    observations: &BTreeMap<Vec<Vec<u8>>, Option<SizeValue>>,
) -> Result<Vec<ResultItem>, BuildError> {
    u32::try_from(observations.len()).map_err(|_| BuildError::CapacityExceeded)?;
    let root = observations
        .get(&Vec::new())
        .ok_or(BuildError::InvalidTree)?;
    let mut by_parent: BTreeMap<Vec<Vec<u8>>, Vec<_>> = BTreeMap::new();
    for (path, size) in observations.iter().filter(|(path, _)| !path.is_empty()) {
        by_parent
            .entry(path[..path.len() - 1].to_vec())
            .or_default()
            .push((path, size));
    }
    let mut items = vec![ResultItem {
        item_no: 0,
        parent: None,
        name: Vec::new(),
        size: root.clone(),
    }];
    let mut paths = vec![Vec::<Vec<u8>>::new()];
    let mut cursor = 0;
    while cursor < paths.len() {
        let path = &paths[cursor];
        let parent = u32::try_from(cursor).map_err(|_| BuildError::CapacityExceeded)?;
        for (child, size) in by_parent.remove(path).unwrap_or_default() {
            let item_no = u32::try_from(items.len()).map_err(|_| BuildError::CapacityExceeded)?;
            items.push(ResultItem {
                item_no,
                parent: Some(parent),
                name: child.last().cloned().ok_or(BuildError::InvalidTree)?,
                size: size.clone(),
            });
            paths.push(child.clone());
        }
        cursor += 1;
    }
    if items.len() != observations.len() {
        return Err(BuildError::InvalidTree);
    }
    Ok(items)
}
