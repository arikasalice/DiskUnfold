//! Job-runtime adapter for collecting an already verified root.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    job::{
        HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobOutcome, ProgressUnit,
        ResourceClass,
    },
    result::{ResultNativePath, ResultScopeRoot},
    scan_observation::OpenedScopeRoot,
};

use crate::scan::{
    DevicePlan, PreMetadataFilter, ResultIds, ScanControlHandle, ScanControlSession,
    collect_open_forest_diagnostic,
};

type DeviceRoots = Vec<(OpenedScopeRoot, Option<Vec<u8>>)>;

const KIND: &str = "scan.metadata";
// Conservative starting concurrency until device-specific policy is composed.
const DEFAULT_SCAN_WORKERS: usize = 4;

/// Describes the base metadata collection job.
pub fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: KIND.into(),
        version: 1,
        display_name_key: KIND.into(),
        controls: vec![JobControl::Pause, JobControl::Resume, JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::MetadataIo],
        history: HistoryMode::Summary,
    }
}

/// Per-scan opt-in evidence for the shared directory-ready deque.
///
/// This is not scan memory or channel occupancy. A result is unavailable until
/// the workers stop, or if collection exits before creating the queue. Initial
/// roots count; worker-local overflow and in-flight frames do not. Use one
/// instance per scan.
#[derive(Default)]
pub struct ScanDiagnostics {
    ready_peak: Mutex<Option<usize>>,
}

impl ScanDiagnostics {
    /// Returns the greatest simultaneous shared-ready deque length, if observed.
    pub fn shared_ready_peak(&self) -> Option<usize> {
        self.ready_peak.lock().ok().and_then(|peak| *peak)
    }

    pub(crate) fn record_ready_peak(&self, value: usize) {
        if let Ok(mut peak) = self.ready_peak.lock() {
            *peak = Some(value);
        }
    }
}

/// Collects from the handle established during scan admission.
pub struct MetadataScan {
    roots: Mutex<Option<DeviceRoots>>,
    store: IngestSink,
    scope: Id,
    lineage: Option<Id>,
    ids: Arc<dyn ResultIds>,
    filter: Option<Arc<PreMetadataFilter<'static>>>,
    shared_table_limit: Option<usize>,
    diagnostics: Option<Arc<ScanDiagnostics>>,
    control: Arc<ScanControlSession>,
}

impl MetadataScan {
    /// Transfers the reviewed handle and its root evidence together to the worker.
    pub fn new(
        root: OpenedScopeRoot,
        store: IngestSink,
        scope: Id,
        ids: Arc<dyn ResultIds>,
    ) -> Self {
        Self::new_roots(vec![root], store, scope, ids)
    }

    /// Transfers all reviewed roots in normalized producer order to one job.
    pub fn new_roots(
        roots: Vec<OpenedScopeRoot>,
        store: IngestSink,
        scope: Id,
        ids: Arc<dyn ResultIds>,
    ) -> Self {
        Self::new_roots_with_physical_keys(
            roots.into_iter().map(|root| (root, None)).collect(),
            store,
            scope,
            ids,
        )
    }

    /// Supply a confirmed physical-device key for each root when available.
    ///
    /// Empty or absent physical keys fall back to the observed volume ID; an
    /// empty volume ID shares one conservative unknown-device bucket. A caller
    /// must not provide an unverified or path-derived physical-device key.
    pub fn new_roots_with_physical_keys(
        roots: Vec<(OpenedScopeRoot, Option<Vec<u8>>)>,
        store: IngestSink,
        scope: Id,
        ids: Arc<dyn ResultIds>,
    ) -> Self {
        let control = Arc::new(ScanControlSession::new(roots.len()));
        Self {
            roots: Mutex::new(Some(roots)),
            store,
            scope,
            lineage: None,
            ids,
            filter: None,
            shared_table_limit: None,
            diagnostics: None,
            control,
        }
    }
    /// Binds this scan to the stable lineage of its verified scope and source.
    pub fn with_lineage(mut self, lineage: Id) -> Self {
        self.lineage = Some(lineage);
        self
    }

    /// Freezes a caller-validated identity-table capacity for this scan.
    pub fn with_shared_table_limit(mut self, limit: usize) -> Self {
        self.shared_table_limit = Some(limit);
        self
    }

    /// Enables in-process diagnostics for this scan only.
    pub fn with_diagnostics(mut self, diagnostics: Arc<ScanDiagnostics>) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Returns a weak control handle bound to this scan's worker scheduler.
    ///
    /// Store the handle with the admitted job reference. A caller must resolve
    /// each region from the same scan's current pinned result before forwarding
    /// its root index and raw relative components. The handle cannot extend the
    /// job or scheduler lifetime.
    pub fn control_handle(&self) -> ScanControlHandle {
        self.control.handle()
    }

    /// Installs an immutable pre-metadata decision for every root of this job.
    pub fn with_pre_metadata_filter(mut self, filter: Arc<PreMetadataFilter<'static>>) -> Self {
        self.filter = Some(filter);
        self
    }
}

impl JobBody for MetadataScan {
    fn kind_id(&self) -> &str {
        KIND
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let roots = self.roots.lock().ok().and_then(|mut roots| roots.take());
        match roots {
            Some(roots) if !roots.is_empty() => {
                let groups = device_groups(&roots);
                let (observed, directories): (Vec<_>, Vec<_>) = roots
                    .into_iter()
                    .map(|(root, _)| {
                        (
                            ResultScopeRoot {
                                path: ResultNativePath {
                                    encoding: root.canonical_path.encoding,
                                    raw: root.canonical_path.raw,
                                },
                                volume_id: root.volume_id,
                                file_id: root.file_id,
                            },
                            root.directory,
                        )
                    })
                    .unzip();
                let build = match &self.lineage {
                    Some(lineage) => self.store.open_forest_in_lineage(
                        self.scope.clone(),
                        observed,
                        lineage.clone(),
                    ),
                    None => self.store.open_forest(self.scope.clone(), observed),
                };
                let build = build.and_then(|mut build| {
                    if let Some(limit) = self.shared_table_limit {
                        build.set_shared_table_limit(limit)?;
                    }
                    Ok(build)
                });
                let devices = DevicePlan { groups, limit: 4 };
                collect_open_forest_diagnostic(
                    build,
                    directories,
                    &*self.ids,
                    &*context,
                    DEFAULT_SCAN_WORKERS,
                    self.filter.as_deref(),
                    (devices, self.diagnostics.as_deref(), Some(&self.control)),
                )
            }
            _ => {
                self.control.finish();
                JobOutcome {
                    counts: Default::default(),
                    completed_scope: false,
                    failure_reason_key: Some("support.internal_error".into()),
                    unprocessed_summary: None,
                    result_version_id: None,
                }
            }
        }
    }
}

#[derive(Hash, PartialEq, Eq)]
enum DeviceKey {
    Unknown,
    Volume(Vec<u8>),
    Physical(Vec<u8>),
}

fn device_groups(roots: &[(OpenedScopeRoot, Option<Vec<u8>>)]) -> Vec<usize> {
    let mut owners = HashMap::new();
    let mut parents: Vec<usize> = (0..roots.len()).collect();
    for (index, (root, physical)) in roots.iter().enumerate() {
        let mut keys = Vec::with_capacity(2);
        if !root.volume_id.is_empty() {
            keys.push(DeviceKey::Volume(root.volume_id.clone()));
        }
        if let Some(key) = physical.as_ref().filter(|key| !key.is_empty()) {
            keys.push(DeviceKey::Physical(key.clone()));
        }
        if keys.is_empty() {
            keys.push(DeviceKey::Unknown);
        }
        for key in keys {
            if let Some(&owner) = owners.get(&key) {
                let left = find(&mut parents, index);
                let right = find(&mut parents, owner);
                parents[left] = right;
            } else {
                owners.insert(key, index);
            }
        }
    }
    (0..roots.len())
        .map(|index| find(&mut parents, index))
        .collect()
}

fn find(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

#[cfg(test)]
mod device_tests {
    use super::*;
    use du_core_contracts::{
        OsPath, PathBytes, PathEncoding,
        ports::{PortError, PortErrorKind},
        scan_observation::{AnchoredDirectory, DirectoryObservations, ObservedEntry},
    };
    struct Empty;
    impl DirectoryObservations for Empty {
        fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
            Ok(Vec::new())
        }
    }
    impl AnchoredDirectory for Empty {
        fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            })
        }
    }
    fn root(volume: &[u8], physical: Option<&[u8]>) -> (OpenedScopeRoot, Option<Vec<u8>>) {
        (
            OpenedScopeRoot {
                canonical_path: OsPath {
                    encoding: PathEncoding::WindowsUtf16le,
                    raw: PathBytes::from_bytes(&[65, 0]),
                    display: "A".into(),
                    lossy: false,
                },
                volume_id: volume.to_vec(),
                file_id: vec![1],
                directory: Box::new(Empty),
            },
            physical.map(<[u8]>::to_vec),
        )
    }
    #[test]
    fn groups_confirmed_physical_and_volume_and_unknown_evidence() {
        let roots = vec![
            root(&[1], Some(&[9])),
            root(&[1], None),
            root(&[2], Some(&[9])),
            root(&[3], None),
            root(&[], None),
            root(&[], None),
        ];
        let groups = device_groups(&roots);
        assert_eq!(groups[0], groups[1]);
        assert_eq!(groups[1], groups[2]);
        assert_ne!(groups[2], groups[3]);
        assert_eq!(groups[4], groups[5]);
        assert_ne!(groups[3], groups[4]);
    }
}
