//! In-memory result publication and conservative per-subtree space aggregation.
mod aggregation_rule;
pub mod browse;
mod cancelled_below;
mod current_pin;
mod directory_case;
pub mod file_type;
pub mod folder_collection;
mod folder_completion;
mod folder_evidence_api;
mod folder_evidence_attempt;
mod folder_evidence_budget;
mod folder_evidence_build_api;
mod folder_evidence_copy;
mod folder_evidence_receiver;
mod folder_evidence_snapshot;
mod folder_evidence_snapshot_builder;
mod folder_evidence_validation;
mod folder_marks;
#[cfg(feature = "test-hooks")]
pub mod test_hooks;
pub use folder_evidence_api::{
    FolderAttemptError, FolderAttemptFinal, FolderAttemptInput, FolderAttemptStatus,
    FolderAttemptToken,
};
pub use folder_evidence_snapshot::FolderAttemptSummary;
#[cfg(test)]
mod folder_evidence_build_handle_tests;
#[cfg(test)]
mod folder_evidence_rule_tests;
#[cfg(test)]
mod folder_evidence_snapshot_budget_tests;
mod forest;
pub mod freshness;
mod freshness_ledger;
mod freshness_outcomes;
pub mod group;
mod issues;
mod item_kind;
mod physical;
pub mod publication_cadence;
mod publication_events;
pub mod query;
mod replacement;
mod result_path;
mod result_read;
mod sample_versions;
mod scope_header;
pub use scope_header::ScopeHeader;
mod scope_observation;
pub use scope_observation::{
    ObservationSkip, ObservationStamp, SealedScanError, fingerprint_from_hex,
};
pub mod size_rule_registry;
pub mod snapshot_load;
pub mod source_registry;
mod space;
pub mod space_detail;
mod space_limit;
pub mod statistics;
pub mod statistics_distribution;
pub mod statistics_summary;
mod tree;
mod version_store;
pub mod visualization;
pub mod volume_gap;
pub use aggregation_rule::{BoundSizeRule, SizeRuleIdentity};
use du_core_contracts::result::{
    ResultCompleteness, ResultItem, ResultScopeGap, ResultScopeRoot, SpaceAggregate,
};
use du_core_contracts::result_events::{ResultVersionChange, ResultVersionPublisher};
use du_core_contracts::{Id, Timestamp};
pub use physical::{FileIdentity, SharingEvidence, SpaceObservation};
use publication_events::PublicationEvents;
use result_read::ensure_directory;
pub use result_read::{ItemDetail, QueryError};
pub use sample_versions::SampleSessionKey;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use version_store::{Entry, State};
mod build_error;
pub use build_error::BuildError;
mod build_abort;
mod build_add;
mod completed_scan;
pub use completed_scan::{CompletedScan, CompletedScanError, ScanCompletion};
mod volume_reports;
pub use volume_reports::{VolumeReport, VolumeReportPoint};
mod observation;
mod observation_range;
mod observation_store;
mod origin;
use observation::StoredObservation;
pub use observation::{Observation, ObservedAttribute};
pub use observation_range::ObservationRange;
use origin::ResultOrigin;
#[derive(Debug)]
pub struct ResultVersion {
    version_id: Id,
    origin: ResultOrigin,
    data_source: du_core_contracts::data_source::DataSource,
    snapshot_id: Option<Id>,
    base_version_id: Option<Id>,
    viz_models: Mutex<visualization::live_models::OwnedModels>,
    scope_version_id: Id,
    lineage_id: Option<Id>,
    scope_fingerprint: Option<String>,
    volume_group: Option<du_core_contracts::volume::RootVolumeGroup>,
    completeness: ResultCompleteness,
    size_rule: BoundSizeRule,
    items: Vec<ResultItem>,
    aggregates: space::SpaceAggregates,
    observation_ranges: Vec<observation_range::RangeIndices>,
    scope_observation_range: observation_range::RangeIndices,
    physical: Option<physical::PhysicalObservations>,
    identity_groups: std::sync::OnceLock<Result<space_detail::IdentityGroups, ()>>,
    shared_table_limit: usize,
    roots: Vec<ResultScopeRoot>,
    root_indices: Vec<u32>,
    gaps: Vec<ResultScopeGap>,
    exact_gaps: BTreeMap<u32, Vec<usize>>,
    directory_case: BTreeMap<u32, du_core_contracts::scan_observation::DirectoryCaseEvidence>,
    folder_marks: folder_marks::FolderMarks,
    scope_total: SpaceAggregate,
    volume_reports: Vec<VolumeReport>,
    observed_at: Option<Timestamp>,
    modified_times: Option<Vec<Option<Timestamp>>>,
    native_modified_times:
        Option<Vec<Option<du_core_contracts::scan_observation::NativeModifiedTime>>>,
    enumerated_inodes: Option<Vec<Option<std::num::NonZeroU64>>>,
    placeholder_observations:
        Option<Vec<Option<du_core_contracts::scan_observation::PlaceholderObservation>>>,
    item_observed_times: Option<Vec<Option<Timestamp>>>,
    issues: Vec<du_core_contracts::scan_failures::ScanIssue>,
    scan_issue_page_index: std::sync::OnceLock<issues::PageIndex>,
    /// Per-root outcomes for the freshness ledger, computed at seal time outside the store
    /// lock. `None` when the version was not sealed by a store that tracks freshness.
    freshness_outcomes: Option<Box<[freshness_outcomes::RootOutcome]>>,
    folder_evidence: Option<folder_evidence_snapshot::FolderEvidenceSnapshot>,
    /// Boundary files (F03 FR5.HD1) and unknown attribute reasons (FR2.HD1, #239).
    sparse: tree::SparseItems,
}

/// A single publication point. Readers retain their snapshot after releasing the lock.
#[derive(Clone, Default)]
pub struct IngestSink {
    state: Arc<Mutex<State>>,
    events: Arc<PublicationEvents>,
    folder_evidence_budget: Arc<folder_evidence_budget::FolderEvidenceBudget>,
}
impl IngestSink {
    /// Creates the result store and binds it to the runtime notification channel.
    pub fn with_version_publisher(publisher: Arc<dyn ResultVersionPublisher>) -> Self {
        Self {
            state: Arc::default(),
            events: Arc::new(PublicationEvents::new(publisher, None)),
            folder_evidence_budget: Arc::default(),
        }
    }
    /// Opens a rootless legacy build for ordinary observations.
    ///
    /// This build has no verified root identity, so `begin_folder_attempt` returns
    /// `FolderAttemptError::InvalidDirectory`; use `open_observed` or `open_forest` instead.
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
        let build_generation = folder_evidence_build_api::next_build_generation()?;
        let mut state = self.state.lock().map_err(|_| BuildError::Unavailable)?;
        let baseline = state.begin(&lineage)?;
        Ok(BuildHandle {
            state: Arc::clone(&self.state),
            events: Arc::clone(&self.events),
            baseline,
            lineage,
            expected_base: None,
            reserved: true,
            origin: ResultOrigin::Scan,
            data_source: du_core_contracts::data_source::DataSource::Real,
            snapshot_id: None,
            published: false,
            size_rule: BoundSizeRule::builtin_default(),
            shared_table_limit: physical::IDENTITY_LIMIT,
            scope_version_id,
            scope_fingerprint: None,
            volume_group: None,
            roots,
            gaps: Vec::new(),
            observations: observation_store::ObservationStore::default(),
            numbered_nodes: None,
            directory_case: BTreeMap::new(),
            unread_folders: BTreeMap::new(),
            issues: Vec::new(),
            volume_reports: Vec::new(),
            finished: false,
            folder_attempts: folder_evidence_receiver::FolderAttemptLedger::new(
                build_generation,
                Arc::clone(&self.folder_evidence_budget),
            ),
        })
    }
}
/// Dropping or aborting an unfinished build withdraws its partial publications.
pub struct BuildHandle {
    state: Arc<Mutex<State>>,
    events: Arc<PublicationEvents>,
    baseline: Option<Entry>,
    lineage: Option<Id>,
    expected_base: Option<Id>,
    reserved: bool,
    origin: ResultOrigin,
    data_source: du_core_contracts::data_source::DataSource,
    snapshot_id: Option<Id>,
    published: bool,
    size_rule: BoundSizeRule,
    shared_table_limit: usize,
    scope_version_id: Id,
    scope_fingerprint: Option<String>,
    volume_group: Option<du_core_contracts::volume::RootVolumeGroup>,
    roots: Vec<ResultScopeRoot>,
    gaps: Vec<ResultScopeGap>,
    observations: observation_store::ObservationStore,
    /// Cached topology order; each publication still rebuilds its result and aggregates.
    numbered_nodes: Option<Arc<Vec<u32>>>,
    /// Case evidence by store node of the directory it was recorded for.
    directory_case: BTreeMap<u32, du_core_contracts::scan_observation::DirectoryCaseEvidence>,
    /// Unread-folder marks by store node of the folder they were reported for.
    unread_folders: BTreeMap<u32, folder_collection::UnreadReason>,
    issues: Vec<du_core_contracts::scan_failures::ScanIssue>,
    volume_reports: Vec<VolumeReport>,
    finished: bool,
    folder_attempts: folder_evidence_receiver::FolderAttemptLedger,
}
impl BuildHandle {
    fn valid_root(&self, root_index: u32) -> bool {
        if self.roots.is_empty() {
            root_index == 0
        } else {
            (root_index as usize) < self.roots.len()
        }
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
        if self.expected_base.is_some() {
            return Err(BuildError::Unavailable);
        }
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
            &mut self.observations,
            self.roots.len().max(1),
            self.roots.len() > 1,
            finish,
            &mut self.numbered_nodes,
        ) {
            Ok(items) => items,
            Err(error) => {
                self.abort();
                return Err(error);
            }
        };
        let folder_evidence = self.pin_folder_evidence(&built, finish)?;
        let proven = folder_completion::ProvenFolders::decide(
            &built,
            folder_evidence.as_ref(),
            completeness,
            self.expected_base.is_some(),
        );
        let limit = self.shared_table_limit;
        let mut aggregates = self
            .size_rule
            .aggregate(&built, completeness, proven.as_ref(), limit);
        let gaps = forest::mark_gaps(&built, &self.observations, &self.gaps, &mut aggregates);
        forest::mark_unread_folders(&built, self.unread_folders.keys(), &mut aggregates);
        let exact_gaps = match gaps {
            Ok(exact) => exact,
            Err(error) => {
                self.abort();
                return Err(error);
            }
        };
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
                self.size_rule
                    .scope_total(&built, result_completeness, self.shared_table_limit);
            if !self.gaps.is_empty() {
                space::mark_inaccessible(
                    &mut total,
                    self.gaps.len() as u64,
                    built.physical.is_some(),
                );
            }
            Some(total)
        };
        let scope_total = match scope_total {
            Some(total) => total,
            None => match aggregates.get(0) {
                Some(root) => root.clone(),
                None => {
                    self.abort();
                    return Err(BuildError::InvalidTree);
                }
            },
        };
        let (observation_ranges, scope_observation_range) =
            observation_range::build(&built, &aggregates, completeness);
        let directory_case = directory_case::bind(&built, &self.directory_case);
        let folder_marks = folder_marks::FolderMarks::bind(
            &built,
            &self.observations,
            &self.issues,
            &self.unread_folders,
            folder_evidence
                .as_ref()
                .filter(|_| self.expected_base.is_none()),
            self.folder_attempts.evidence_dropped(),
            finish,
        );
        // Linear in items and gaps, so it runs here and never under the state lock below.
        let freshness_outcomes = (finish && self.events.tracks_freshness()).then(|| {
            freshness_outcomes::compute(
                self.roots.len(),
                result_completeness,
                &built.items,
                &built.root_indices,
                &self.gaps,
            )
        });
        let result = Arc::new(ResultVersion {
            version_id,
            origin: self.origin,
            data_source: self.data_source,
            snapshot_id: self.snapshot_id.clone(),
            base_version_id: self.expected_base.clone(),
            viz_models: Mutex::default(),
            scope_version_id: self.scope_version_id.clone(),
            lineage_id: self.lineage.clone(),
            scope_fingerprint: self.scope_fingerprint.clone(),
            volume_group: self.volume_group.clone(),
            completeness: result_completeness,
            size_rule: self.size_rule,
            items: built.items,
            physical: built.physical,
            identity_groups: std::sync::OnceLock::new(),
            shared_table_limit: self.shared_table_limit,
            modified_times: built.modified_times,
            native_modified_times: built.native_modified_times,
            enumerated_inodes: built.enumerated_inodes,
            placeholder_observations: built.placeholder_observations,
            item_observed_times: built.item_observed_times,
            issues: issues::ordered(&self.issues),
            scan_issue_page_index: std::sync::OnceLock::new(),
            aggregates,
            observation_ranges,
            scope_observation_range,
            roots: self.roots.clone(),
            root_indices: built.root_indices,
            gaps: self.gaps.clone(),
            exact_gaps,
            directory_case,
            folder_marks,
            scope_total,
            volume_reports: self.volume_reports.clone(),
            observed_at,
            freshness_outcomes,
            folder_evidence,
            sparse: built.sparse,
        });
        let order = self
            .events
            .order
            .lock()
            .map_err(|_| BuildError::Unavailable)?;
        let mut state = self.state.lock().map_err(|_| BuildError::Unavailable)?;
        let previous = match state.publish(
            &self.lineage,
            Arc::clone(&result),
            finish,
            self.expected_base.as_ref(),
            self.reserved,
        ) {
            Ok(previous) => previous,
            Err(error) => {
                drop(state);
                drop(order);
                self.abort();
                return Err(error);
            }
        };
        let was_published = std::mem::replace(&mut self.published, true);
        if finish {
            self.finished = true;
        }
        let sequence = state.next_event_sequence(self.lineage.as_ref());
        // Only O(roots) work may happen under this lock; query admissions wait on it.
        let freshness = if self.events.tracks_freshness() {
            state.freshness_published(&self.lineage, &result, !was_published, finish)
        } else {
            Vec::new()
        };
        drop(state);
        self.events.notify(
            self.lineage.as_ref(),
            sequence,
            ResultVersionChange::Published,
            previous.as_ref().map(|entry| entry.result.as_ref()),
            Some(&result),
        );
        self.events.notify_freshness(freshness);
        drop(order);
        // Retiring raises no notification and the replaced version is already not current,
        // so registrations of it fail as stale; it needs no place in the publication order.
        self.retire_replaced(previous.as_ref(), was_published, finish);
        drop(previous);
        Ok(result)
    }
}
