//! In-memory result publication and conservative per-subtree space aggregation.
pub mod browse;
mod forest;
mod physical;
pub mod publication_cadence;
pub mod query;
mod result_path;
mod result_read;
mod space;
mod tree;
mod version_store;
pub mod visualization;
use du_core_contracts::result::{
    ResultCompleteness, ResultItem, ResultItemKind, ResultScopeGap, ResultScopeRoot, SpaceAggregate,
};
use du_core_contracts::{Id, SizeValue, Timestamp};
pub use physical::{FileIdentity, SharingEvidence, SpaceObservation};
pub use result_read::ItemDetail;
use result_read::ensure_directory;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use version_store::State;

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
    lineage_id: Option<Id>,
    completeness: ResultCompleteness,
    items: Vec<ResultItem>,
    aggregates: Vec<SpaceAggregate>,
    physical: Option<Vec<Option<SpaceObservation>>>,
    roots: Vec<ResultScopeRoot>,
    root_indices: Vec<u32>,
    gaps: Vec<ResultScopeGap>,
    scope_total: Option<SpaceAggregate>,
    observed_at: Option<Timestamp>,
}

/// Failure to resolve an item or a current version without changing the pinned result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryError {
    NotFound,
    StaleVersion,
    InvalidPath,
    PathUnavailable,
    NotDirectory,
    KindUnknown,
    Unavailable,
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
        state.pin_current(expected)
    }
    /// Pins the most recently published current result, when present.
    pub fn pin(&self) -> Result<Option<Arc<ResultVersion>>, BuildError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .pin())
    }

    /// Returns the stable current-analysis lineage for a canonical scope fingerprint.
    /// The caller supplies a unique candidate ID; an existing binding is reused.
    pub fn bind_current_analysis(
        &self,
        fingerprint: &str,
        candidate: Id,
    ) -> Result<Id, BuildError> {
        self.state
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .bind(fingerprint, candidate)
    }

    pub fn open(&self, scope_version_id: Id) -> Result<BuildHandle, BuildError> {
        self.begin(scope_version_id, Vec::new(), None)
    }

    /// Opens a build whose publications all retain the same verified root evidence.
    pub fn open_observed(
        &self,
        scope_version_id: Id,
        root: ResultScopeRoot,
    ) -> Result<BuildHandle, BuildError> {
        self.begin(scope_version_id, vec![root], None)
    }

    /// Opens one build for actual roots in producer order, without a synthetic item.
    /// Root indices are zero-based positions in `roots` and stay fixed during the build.
    pub fn open_forest(
        &self,
        scope_version_id: Id,
        roots: Vec<ResultScopeRoot>,
    ) -> Result<BuildHandle, BuildError> {
        if roots.is_empty() || u32::try_from(roots.len()).is_err() {
            return Err(BuildError::InvalidTree);
        }
        self.begin(scope_version_id, roots, None)
    }

    /// Opens a forest in a previously bound current-analysis lineage.
    pub fn open_forest_in_lineage(
        &self,
        scope_version_id: Id,
        roots: Vec<ResultScopeRoot>,
        lineage: Id,
    ) -> Result<BuildHandle, BuildError> {
        if roots.is_empty() || u32::try_from(roots.len()).is_err() {
            return Err(BuildError::InvalidTree);
        }
        self.begin(scope_version_id, roots, Some(lineage))
    }

    fn begin(
        &self,
        scope_version_id: Id,
        roots: Vec<ResultScopeRoot>,
        lineage: Option<Id>,
    ) -> Result<BuildHandle, BuildError> {
        let mut state = self.state.lock().map_err(|_| BuildError::Unavailable)?;
        let baseline = state.begin(&lineage)?;
        Ok(BuildHandle {
            state: Arc::clone(&self.state),
            baseline,
            lineage,
            focus_before: None,
            published: false,
            scope_version_id,
            roots,
            gaps: Vec::new(),
            observations: BTreeMap::new(),
            finished: false,
        })
    }
}

/// Dropping or aborting an unfinished build withdraws its partial publications.
pub struct BuildHandle {
    state: Arc<Mutex<State>>,
    baseline: Option<Arc<ResultVersion>>,
    lineage: Option<Id>,
    focus_before: Option<Option<Id>>,
    published: bool,
    scope_version_id: Id,
    roots: Vec<ResultScopeRoot>,
    gaps: Vec<ResultScopeGap>,
    observations: BTreeMap<(u32, Vec<Vec<u8>>), StoredObservation>,
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

    /// Adds a confirmed file even if its logical size could not be read.
    pub fn add_file(&mut self, observation: Observation) -> Result<(), BuildError> {
        self.insert(
            observation.path,
            StoredObservation {
                size: observation.size,
                kind: ResultItemKind::File,
                physical: None,
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
        self.insert_at(0, path, observation)
    }

    fn valid_root(&self, root_index: u32) -> bool {
        if self.roots.is_empty() {
            root_index == 0
        } else {
            (root_index as usize) < self.roots.len()
        }
    }

    fn insert_at(
        &mut self,
        root_index: u32,
        path: Vec<Vec<u8>>,
        observation: StoredObservation,
    ) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        if !self.valid_root(root_index)
            || path.iter().any(Vec::is_empty)
            || self.observations.contains_key(&(root_index, path.clone()))
        {
            self.abort();
            return Err(BuildError::InvalidTree);
        }
        self.observations.insert((root_index, path), observation);
        Ok(())
    }
    pub fn publish_partial(&mut self, version_id: Id) -> Result<Arc<ResultVersion>, BuildError> {
        self.publish_partial_at(version_id, None)
    }

    /// Publishes a partial snapshot with the collector's observation time, when available.
    pub fn publish_partial_at(
        &mut self,
        version_id: Id,
        observed_at: Option<Timestamp>,
    ) -> Result<Arc<ResultVersion>, BuildError> {
        self.publish(version_id, ResultCompleteness::Partial, observed_at, false)
    }
    pub fn seal(
        self,
        version_id: Id,
        completeness: ResultCompleteness,
    ) -> Result<Arc<ResultVersion>, BuildError> {
        self.seal_at(version_id, completeness, None)
    }

    /// Seals the final snapshot using the collector's observation time, when available.
    pub fn seal_at(
        mut self,
        version_id: Id,
        completeness: ResultCompleteness,
        observed_at: Option<Timestamp>,
    ) -> Result<Arc<ResultVersion>, BuildError> {
        self.publish(version_id, completeness, observed_at, true)
    }
    fn publish(
        &mut self,
        version_id: Id,
        completeness: ResultCompleteness,
        observed_at: Option<Timestamp>,
        finish: bool,
    ) -> Result<Arc<ResultVersion>, BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        let built = match tree::build_items(
            &self.observations,
            self.roots.len().max(1),
            self.roots.len() > 1,
        ) {
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
        if let Err(error) = forest::mark_gaps(&built, &self.gaps, &mut aggregates) {
            self.abort();
            return Err(error);
        }
        let result_completeness =
            if !self.gaps.is_empty() && completeness == ResultCompleteness::Complete {
                ResultCompleteness::Missing
            } else {
                completeness
            };
        let scope_total = if self.roots.len() <= 1 {
            None
        } else {
            let mut total =
                space::scope_total(&built.items, &built.structures, result_completeness);
            physical::scope_total(
                &built.items,
                &built.structures,
                built.physical.as_deref(),
                result_completeness,
                &mut total,
            );
            if !self.gaps.is_empty() {
                space::mark_inaccessible(&mut total, self.gaps.len() as u64);
            }
            Some(total)
        };
        let result = Arc::new(ResultVersion {
            version_id,
            scope_version_id: self.scope_version_id.clone(),
            lineage_id: self.lineage.clone(),
            completeness: result_completeness,
            items: built.items,
            physical: built.physical,
            aggregates,
            roots: self.roots.clone(),
            root_indices: built.keys.into_iter().map(|(root, _)| root).collect(),
            gaps: self.gaps.clone(),
            scope_total,
            observed_at,
        });
        let mut state = self.state.lock().map_err(|_| BuildError::Unavailable)?;
        let (focus_before, previous) =
            match state.publish(&self.lineage, Arc::clone(&result), finish) {
                Ok(focus) => focus,
                Err(error) => {
                    drop(state);
                    self.abort();
                    return Err(error);
                }
            };
        if !self.published || focus_before.as_ref() != Some(&self.lineage) {
            self.focus_before = focus_before;
        }
        self.published = true;
        if finish {
            self.finished = true;
        }
        drop(state);
        drop(previous);
        Ok(result)
    }
    pub fn abort(&mut self) {
        if !self.finished {
            let withdrawn = self.state.lock().ok().and_then(|mut state| {
                state.abort(
                    &self.lineage,
                    self.baseline.clone(),
                    self.published,
                    self.focus_before.clone(),
                )
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
