//! In-memory result publication and conservative per-subtree space aggregation.
pub mod browse;
mod forest;
mod issues;
mod physical;
pub mod publication_cadence;
mod publication_events;
pub mod query;
mod result_path;
mod result_read;
mod space;
pub mod space_detail;
mod space_limit;
pub mod statistics;
mod tree;
mod version_store;
pub mod visualization;
pub mod volume_gap;
use du_core_contracts::result::{
    ResultCompleteness, ResultItem, ResultItemKind, ResultScopeGap, ResultScopeRoot, SpaceAggregate,
};
use du_core_contracts::result_events::{ResultVersionChange, ResultVersionPublisher};
use du_core_contracts::{Id, SizeValue, Timestamp};
pub use physical::{FileIdentity, SharingEvidence, SpaceObservation};
use publication_events::PublicationEvents;
use result_read::ensure_directory;
pub use result_read::{ItemDetail, QueryError};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use version_store::{Entry, State};
mod build_error;
pub use build_error::BuildError;
mod volume_reports;
pub use volume_reports::{VolumeReport, VolumeReportPoint};
mod observation;
pub use observation::Observation;
#[derive(Debug)]
pub struct ResultVersion {
    version_id: Id,
    viz_models: Mutex<visualization::live_models::OwnedModels>,
    scope_version_id: Id,
    lineage_id: Option<Id>,
    completeness: ResultCompleteness,
    items: Vec<ResultItem>,
    aggregates: Vec<SpaceAggregate>,
    physical: Option<Vec<Option<SpaceObservation>>>,
    identity_groups: std::sync::OnceLock<Result<space_detail::IdentityGroups, ()>>,
    shared_table_limit: usize,
    roots: Vec<ResultScopeRoot>,
    root_indices: Vec<u32>,
    gaps: Vec<ResultScopeGap>,
    scope_total: Option<SpaceAggregate>,
    volume_reports: Vec<VolumeReport>,
    observed_at: Option<Timestamp>,
    modified_times: Option<Vec<Option<Timestamp>>>,
    item_observed_times: Option<Vec<Option<Timestamp>>>,
    issues: Vec<du_core_contracts::scan_failures::ScanIssue>,
}
/// A single publication point. Readers retain their snapshot after releasing the lock.
#[derive(Clone, Default)]
pub struct IngestSink {
    state: Arc<Mutex<State>>,
    events: Arc<PublicationEvents>,
}
impl IngestSink {
    /// Creates the result store and binds it to the runtime notification channel.
    pub fn with_version_publisher(publisher: Arc<dyn ResultVersionPublisher>) -> Self {
        Self {
            state: Arc::default(),
            events: Arc::new(PublicationEvents::new(publisher)),
        }
    }
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
            events: Arc::clone(&self.events),
            baseline,
            lineage,
            published: false,
            shared_table_limit: physical::IDENTITY_LIMIT,
            scope_version_id,
            roots,
            gaps: Vec::new(),
            observations: BTreeMap::new(),
            issues: Vec::new(),
            volume_reports: Vec::new(),
            finished: false,
        })
    }
}

/// Dropping or aborting an unfinished build withdraws its partial publications.
pub struct BuildHandle {
    state: Arc<Mutex<State>>,
    events: Arc<PublicationEvents>,
    baseline: Option<Entry>,
    lineage: Option<Id>,
    published: bool,
    shared_table_limit: usize,
    scope_version_id: Id,
    roots: Vec<ResultScopeRoot>,
    gaps: Vec<ResultScopeGap>,
    observations: BTreeMap<(u32, Vec<Vec<u8>>), StoredObservation>,
    issues: Vec<du_core_contracts::scan_failures::ScanIssue>,
    volume_reports: Vec<VolumeReport>,
    finished: bool,
}
struct StoredObservation {
    size: Option<SizeValue>,
    kind: ResultItemKind,
    physical: Option<Box<SpaceObservation>>,
    modified_at: Option<Timestamp>,
    observed_at: Option<Timestamp>,
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
                modified_at: None,
                observed_at: None,
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
                modified_at: None,
                observed_at: None,
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
                modified_at: None,
                observed_at: None,
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
                modified_at: None,
                observed_at: None,
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
                modified_at: None,
                observed_at: None,
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
                modified_at: None,
                observed_at: None,
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
            self.shared_table_limit,
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
                self.shared_table_limit,
                &mut total,
            );
            if !self.gaps.is_empty() {
                space::mark_inaccessible(&mut total, self.gaps.len() as u64);
            }
            Some(total)
        };
        let result = Arc::new(ResultVersion {
            version_id,
            viz_models: Mutex::default(),
            scope_version_id: self.scope_version_id.clone(),
            lineage_id: self.lineage.clone(),
            completeness: result_completeness,
            items: built.items,
            physical: built.physical,
            identity_groups: std::sync::OnceLock::new(),
            shared_table_limit: self.shared_table_limit,
            modified_times: built.modified_times,
            item_observed_times: built.item_observed_times,
            issues: issues::ordered(&self.issues),
            aggregates,
            roots: self.roots.clone(),
            root_indices: built.keys.into_iter().map(|(root, _)| root).collect(),
            gaps: self.gaps.clone(),
            scope_total,
            volume_reports: self.volume_reports.clone(),
            observed_at,
        });
        let order = self
            .events
            .order
            .lock()
            .map_err(|_| BuildError::Unavailable)?;
        let mut state = self.state.lock().map_err(|_| BuildError::Unavailable)?;
        let previous = match state.publish(&self.lineage, Arc::clone(&result), finish) {
            Ok(previous) => previous,
            Err(error) => {
                drop(state);
                drop(order);
                self.abort();
                return Err(error);
            }
        };
        self.published = true;
        if finish {
            self.finished = true;
        }
        let sequence = state.next_event_sequence(self.lineage.as_ref());
        drop(state);
        self.events.notify(
            self.lineage.as_ref(),
            sequence,
            ResultVersionChange::Published,
            previous.as_ref().map(|entry| entry.result.as_ref()),
            Some(&result),
        );
        drop(order);
        drop(previous);
        Ok(result)
    }
    pub fn abort(&mut self) {
        if !self.finished {
            self.finished = true;
            let order = self
                .events
                .order
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let withdrawn = state.abort(&self.lineage, self.baseline.clone(), self.published);
            let sequence = withdrawn
                .as_ref()
                .and_then(|_| state.next_event_sequence(self.lineage.as_ref()));
            drop(state);
            self.events.notify(
                self.lineage.as_ref(),
                sequence,
                ResultVersionChange::Withdrawn,
                withdrawn.as_ref().map(|entry| entry.result.as_ref()),
                self.baseline.as_ref().map(|entry| entry.result.as_ref()),
            );
            drop(order);
            drop(withdrawn);
        }
    }
}
impl Drop for BuildHandle {
    fn drop(&mut self) {
        self.abort();
    }
}
