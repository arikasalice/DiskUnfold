//! In-memory result publication and conservative per-subtree space aggregation.
mod physical;
mod space;
use du_core_contracts::result::{
    ResultCompleteness, ResultItem, ResultItemKind, ResultVersionRead, SpaceAggregate,
};
use du_core_contracts::{Id, ItemRef, SizeValue};
pub use physical::{FileIdentity, SharingEvidence, SpaceObservation};
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
    physical: Option<Vec<Option<SpaceObservation>>>,
}

/// Failure to resolve an item or a current version without changing the pinned result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryError {
    NotFound,
    StaleVersion,
    InvalidPath,
    NotDirectory,
    KindUnknown,
    Unavailable,
}

/// Original observations for one item in a pinned result, without inferring missing values.
pub struct ItemDetail<'a> {
    pub item: &'a ResultItem,
    pub physical: Option<&'a SpaceObservation>,
}

impl ResultVersion {
    /// Returns the aggregate for an item in this pinned version, or `None` for an invalid number.
    pub fn space(&self, item_no: u32) -> Option<&SpaceAggregate> {
        self.aggregates.get(item_no as usize)
    }

    /// Resolves an item reference against this snapshot, including an older snapshot held open.
    pub fn detail(&self, item: &ItemRef) -> Result<ItemDetail<'_>, QueryError> {
        if item.result_version != self.version_id {
            return Err(QueryError::StaleVersion);
        }
        let index = item.item_no as usize;
        let item = self.items.get(index).ok_or(QueryError::NotFound)?;
        let physical = self
            .physical
            .as_ref()
            .and_then(|values| values.get(index))
            .and_then(Option::as_ref);
        Ok(ItemDetail { item, physical })
    }

    /// Resolves a raw root-relative path to a confirmed directory's aggregate in this snapshot.
    pub fn folder(&self, version: &Id, path: &[Vec<u8>]) -> Result<&SpaceAggregate, QueryError> {
        if version != &self.version_id {
            return Err(QueryError::StaleVersion);
        }
        if path.iter().any(Vec::is_empty) {
            return Err(QueryError::InvalidPath);
        }
        let mut index = 0usize;
        for component in path {
            ensure_directory(self.items[index].kind)?;
            let parent = u32::try_from(index).map_err(|_| QueryError::NotFound)?;
            let first = self
                .items
                .partition_point(|item| item.parent < Some(parent));
            let end = self
                .items
                .partition_point(|item| item.parent <= Some(parent));
            let offset = self.items[first..end]
                .binary_search_by(|item| item.name.cmp(component))
                .map_err(|_| QueryError::NotFound)?;
            index = first + offset;
        }
        ensure_directory(self.items[index].kind)?;
        self.aggregates.get(index).ok_or(QueryError::NotFound)
    }
}

fn ensure_directory(kind: ResultItemKind) -> Result<(), QueryError> {
    match kind {
        ResultItemKind::Directory => Ok(()),
        ResultItemKind::Unknown | ResultItemKind::UnknownStructure => Err(QueryError::KindUnknown),
        ResultItemKind::File | ResultItemKind::UntraversedLink => Err(QueryError::NotDirectory),
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
    /// Pins the current version only if its identifier still matches the admitted request.
    pub fn pin_current(&self, expected: &Id) -> Result<Arc<ResultVersion>, QueryError> {
        let state = self.state.lock().map_err(|_| QueryError::Unavailable)?;
        let current = state.current.as_ref().ok_or(QueryError::NotFound)?;
        if current.version_id != *expected {
            return Err(QueryError::StaleVersion);
        }
        Ok(Arc::clone(current))
    }
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
    observations: BTreeMap<Vec<Vec<u8>>, StoredObservation>,
    finished: bool,
}

struct StoredObservation {
    size: Option<SizeValue>,
    kind: ResultItemKind,
    physical: Option<Box<SpaceObservation>>,
}

impl BuildHandle {
    /// Adds a confirmed directory with no independent logical-size contribution.
    pub fn add_directory(&mut self, path: Vec<Vec<u8>>) -> Result<(), BuildError> {
        self.insert(
            path,
            StoredObservation {
                size: None,
                kind: ResultItemKind::Directory,
                physical: None,
            },
        )
    }

    /// Adds a link whose target was not traversed; its unknown contribution propagates as missing.
    pub fn add_untraversed_link(&mut self, path: Vec<Vec<u8>>) -> Result<(), BuildError> {
        self.insert(
            path,
            StoredObservation {
                size: None,
                kind: ResultItemKind::UntraversedLink,
                physical: None,
            },
        )
    }
    /// Adds a file and its separately observed physical-space evidence.
    ///
    /// Identity must be an OS-provided volume/file pair, never a path or content hash.
    /// Missing or inconsistent evidence is retained as unknown during aggregation.
    pub fn add_with_space(
        &mut self,
        observation: Observation,
        space: SpaceObservation,
    ) -> Result<(), BuildError> {
        self.insert(
            observation.path,
            StoredObservation {
                size: observation.size,
                kind: ResultItemKind::File,
                physical: Some(Box::new(space)),
            },
        )
    }
    /// Adds a root, directory, or deliberately excluded link with no logical byte contribution.
    ///
    /// This does not indicate that an unread directory was successfully enumerated;
    /// the caller must preserve any omitted work in the result completeness.
    pub fn add_structure(&mut self, path: Vec<Vec<u8>>) -> Result<(), BuildError> {
        self.insert(
            path,
            StoredObservation {
                size: None,
                kind: ResultItemKind::UnknownStructure,
                physical: None,
            },
        )
    }

    pub fn add(&mut self, observation: Observation) -> Result<(), BuildError> {
        let kind = if observation.size.is_some() {
            ResultItemKind::File
        } else {
            ResultItemKind::Unknown
        };
        self.insert(
            observation.path,
            StoredObservation {
                size: observation.size,
                kind,
                physical: None,
            },
        )
    }

    fn insert(
        &mut self,
        path: Vec<Vec<u8>>,
        observation: StoredObservation,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        if path.iter().any(Vec::is_empty) || self.observations.contains_key(&path) {
            self.abort();
            return Err(BuildError::InvalidTree);
        }
        self.observations.insert(path, observation);
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
        let built = match build_items(&self.observations) {
            Ok(items) => items,
            Err(error) => {
                self.abort();
                return Err(error);
            }
        };
        let mut aggregates = space::aggregate(&built.items, &built.structures, completeness);
        physical::apply(
            &built.items,
            &built.structures,
            built.physical.as_deref(),
            completeness,
            &mut aggregates,
        );
        let result = Arc::new(ResultVersion {
            version_id,
            scope_version_id: self.scope_version_id.clone(),
            completeness,
            items: built.items,
            physical: built.physical,
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

struct BuiltItems {
    items: Vec<ResultItem>,
    structures: Vec<bool>,
    physical: Option<Vec<Option<SpaceObservation>>>,
}

fn build_items(
    observations: &BTreeMap<Vec<Vec<u8>>, StoredObservation>,
) -> Result<BuiltItems, BuildError> {
    u32::try_from(observations.len()).map_err(|_| BuildError::CapacityExceeded)?;
    let root = observations
        .get(&Vec::new())
        .ok_or(BuildError::InvalidTree)?;
    let mut by_parent: BTreeMap<Vec<Vec<u8>>, Vec<_>> = BTreeMap::new();
    for (path, observation) in observations.iter().filter(|(path, _)| !path.is_empty()) {
        by_parent
            .entry(path[..path.len() - 1].to_vec())
            .or_default()
            .push((path, observation));
    }
    let mut items = vec![ResultItem {
        item_no: 0,
        parent: None,
        name: Vec::new(),
        size: root.size.clone(),
        kind: root.kind,
    }];
    let mut paths = vec![Vec::<Vec<u8>>::new()];
    let mut structure_flags = vec![is_structure(root.kind)];
    let mut physical_values = observations
        .values()
        .any(|v| v.physical.is_some())
        .then(|| vec![root.physical.as_deref().cloned()]);
    let mut cursor = 0;
    while cursor < paths.len() {
        let path = &paths[cursor];
        let parent = u32::try_from(cursor).map_err(|_| BuildError::CapacityExceeded)?;
        for (child, observation) in by_parent.remove(path).unwrap_or_default() {
            let item_no = u32::try_from(items.len()).map_err(|_| BuildError::CapacityExceeded)?;
            items.push(ResultItem {
                item_no,
                parent: Some(parent),
                name: child.last().cloned().ok_or(BuildError::InvalidTree)?,
                size: observation.size.clone(),
                kind: observation.kind,
            });
            paths.push(child.clone());
            structure_flags.push(is_structure(observation.kind));
            if let Some(values) = &mut physical_values {
                values.push(observation.physical.as_deref().cloned());
            }
        }
        cursor += 1;
    }
    if items.len() != observations.len() {
        return Err(BuildError::InvalidTree);
    }
    Ok(BuiltItems {
        items,
        structures: structure_flags,
        physical: physical_values,
    })
}

fn is_structure(kind: ResultItemKind) -> bool {
    matches!(
        kind,
        ResultItemKind::Directory | ResultItemKind::UnknownStructure
    )
}
