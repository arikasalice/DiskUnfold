//! Job-runtime adapter for collecting an already verified root.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use du_analysis_index::{BoundSizeRule, IngestSink, ScopeHeader};
use du_core_contracts::{
    Id,
    analysis_ipc::ScanResourceMode,
    job::{
        HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobOutcome, ProgressUnit,
        ResourceClass,
    },
    result::{ResultNativePath, ResultScopeRoot},
    scan_observation::{DeviceKind, DeviceKindSource, OpenedScopeRoot},
};

use crate::scan::{
    DevicePlan, LaneDevice, PreMetadataFilter, ResultIds, ScanControlHandle, ScanControlSession,
    collect_open_forest_diagnostic, internal_failure,
};

type DeviceRoots = Vec<(OpenedScopeRoot, RootDevice)>;

const KIND: &str = "scan.metadata";
// Conservative starting concurrency until device-specific policy is composed.
const DEFAULT_SCAN_WORKERS: usize = 4;

/// Device evidence captured from one already-opened scope root.
#[derive(Clone)]
pub struct RootDevice {
    /// Reported storage class, or unknown when detection was unavailable.
    pub kind: DeviceKind,
    /// Source of the storage class supplied to the lane.
    pub source: DeviceKindSource,
    /// Opaque physical disk identity used only for in-process permit sharing.
    pub disk_key: Option<Vec<u8>>,
}

impl RootDevice {
    /// Unknown device evidence for callers without an OS observation.
    pub fn unknown() -> Self {
        Self {
            kind: DeviceKind::Unknown,
            source: DeviceKindSource::Unknown,
            disk_key: None,
        }
    }
}

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

/// Per-scan opt-in evidence for the directory queues of the scan's lanes.
///
/// This is not scan memory or channel occupancy. A result is unavailable until
/// the workers stop, or if collection exits before creating the queue. Initial
/// roots count; worker overflow stacks and in-flight frames do not. Use one
/// instance per scan.
#[derive(Default)]
pub struct ScanDiagnostics {
    ready_peak: Mutex<Option<usize>>,
    ended_folder_attempts: std::sync::atomic::AtomicU64,
}

impl ScanDiagnostics {
    /// Returns the greatest number of items held in all lane queues at once, if observed.
    pub fn shared_ready_peak(&self) -> Option<usize> {
        self.ready_peak.lock().ok().and_then(|peak| *peak)
    }

    /// The folder attempts whose correlated evidence reached the result build and ended
    /// there, once the workers stopped (F02 DLD IF-19, #218).
    pub fn ended_folder_attempts(&self) -> u64 {
        self.ended_folder_attempts
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn record_ready_peak(&self, value: usize) {
        if let Ok(mut peak) = self.ready_peak.lock() {
            *peak = Some(value);
        }
    }

    pub(crate) fn record_ended_folder_attempts(&self, value: u64) {
        self.ended_folder_attempts
            .store(value, std::sync::atomic::Ordering::Release);
    }
}

/// Collects from the handle established during scan admission.
pub struct MetadataScan {
    roots: Mutex<Option<DeviceRoots>>,
    store: IngestSink,
    scope: Id,
    scope_fingerprint: Option<String>,
    lineage: Option<Id>,
    ids: Arc<dyn ResultIds>,
    filter: Option<Arc<PreMetadataFilter<'static>>>,
    shared_table_limit: Option<usize>,
    size_rule: Option<BoundSizeRule>,
    diagnostics: Option<Arc<ScanDiagnostics>>,
    root_count: usize,
    control: Option<Arc<ScanControlSession>>,
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
        Self::new_roots_with_devices(
            roots
                .into_iter()
                .map(|root| (root, RootDevice::unknown()))
                .collect(),
            store,
            scope,
            ids,
        )
    }

    /// Transfers roots and their already observed device evidence into one job.
    pub fn new_roots_with_devices(
        roots: Vec<(OpenedScopeRoot, RootDevice)>,
        store: IngestSink,
        scope: Id,
        ids: Arc<dyn ResultIds>,
    ) -> Self {
        let root_count = roots.len();
        Self {
            roots: Mutex::new(Some(roots)),
            store,
            scope,
            scope_fingerprint: None,
            lineage: None,
            ids,
            filter: None,
            shared_table_limit: None,
            size_rule: None,
            diagnostics: None,
            root_count,
            control: None,
        }
    }

    /// Greatest process-wide device permit after grouping these roots by volume.
    pub fn max_device_permit(roots: &[(Vec<u8>, RootDevice)]) -> u32 {
        lane_devices(roots)
            .1
            .iter()
            .map(|lane| du_job_runtime::device_permit(lane.kind))
            .max()
            .unwrap_or_else(|| du_job_runtime::device_permit(DeviceKind::Unknown))
    }
    /// Binds this scan to the stable lineage of its verified scope and source.
    pub fn with_lineage(mut self, lineage: Id) -> Self {
        self.lineage = Some(lineage);
        self
    }

    /// Carries the fingerprint of the frozen scope in every result header.
    ///
    /// The caller passes the canonical fingerprint computed for the same scope
    /// version that this scan collects (FR1.HD1.app-host.DD1).
    pub fn with_scope_fingerprint(mut self, fingerprint: String) -> Self {
        self.scope_fingerprint = Some(fingerprint);
        self
    }

    /// Freezes a caller-validated identity-table capacity for this scan.
    pub fn with_shared_table_limit(mut self, limit: usize) -> Self {
        self.shared_table_limit = Some(limit);
        self
    }

    /// Binds the size aggregation rule every publication of this scan uses.
    ///
    /// The caller resolves the rule from its registry once, when the analysis
    /// starts, so that the result version reports the rule the scan was admitted
    /// with. Without it the build uses the crate's default rule.
    pub fn with_size_rule(mut self, rule: BoundSizeRule) -> Self {
        self.size_rule = Some(rule);
        self
    }

    /// Enables in-process diagnostics for this scan only.
    pub fn with_diagnostics(mut self, diagnostics: Arc<ScanDiagnostics>) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Enables directory-priority control for this metadata job.
    ///
    /// The opt-in counts outstanding directory work by path for the live
    /// scheduler; that count grows with the queued and running directories.
    /// Jobs that do not expose priority control avoid that work.
    pub fn with_priority_control(mut self) -> Self {
        if self.control.is_none() {
            self.control = Some(Arc::new(ScanControlSession::new(self.root_count)));
        }
        if let Some(control) = &self.control {
            control.enable_priority();
        }
        self
    }

    /// Binds the initial resource condition and creates the weak live control handle.
    pub fn with_resource_condition(
        mut self,
        mode: ScanResourceMode,
        user_limit: Option<u32>,
    ) -> Self {
        if let Some(control) = &self.control {
            control.initialize_resource_condition(mode, user_limit);
        } else {
            self.control = Some(Arc::new(ScanControlSession::with_resource_condition(
                self.root_count,
                mode,
                user_limit,
            )));
        }
        self
    }

    /// Returns a weak control handle bound to this scan's worker scheduler.
    ///
    /// Store the handle with the admitted job reference. A caller must resolve
    /// each region from the same scan's current pinned result before forwarding
    /// its root index and raw relative components. The handle cannot extend the
    /// job or scheduler lifetime.
    pub fn control_handle(&self) -> Option<ScanControlHandle> {
        self.control.as_ref().map(ScanControlSession::handle)
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
                let volumes: Vec<_> = roots
                    .iter()
                    .map(|(root, device)| (root.volume_id.clone(), device.clone()))
                    .collect();
                let (groups, lane_devices) = lane_devices(&volumes);
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
                let build = match (&self.scope_fingerprint, &self.lineage) {
                    (Some(fingerprint), lineage) => self.store.open_scoped_forest(
                        ScopeHeader {
                            version_id: self.scope.clone(),
                            fingerprint: fingerprint.clone(),
                        },
                        observed,
                        lineage.clone(),
                    ),
                    (None, Some(lineage)) => self.store.open_forest_in_lineage(
                        self.scope.clone(),
                        observed,
                        lineage.clone(),
                    ),
                    (None, None) => self.store.open_forest(self.scope.clone(), observed),
                };
                let build = build.and_then(|mut build| {
                    if let Some(limit) = self.shared_table_limit {
                        build.set_shared_table_limit(limit)?;
                    }
                    if let Some(rule) = self.size_rule {
                        build.set_size_rule(rule)?;
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
                    (
                        devices,
                        self.diagnostics.as_deref(),
                        self.control.as_deref(),
                        Some(lane_devices),
                    ),
                )
            }
            _ => {
                if let Some(control) = &self.control {
                    control.finish();
                }
                internal_failure(Default::default())
            }
        }
    }
}

fn lane_devices(roots: &[(Vec<u8>, RootDevice)]) -> (Vec<usize>, Vec<LaneDevice>) {
    let mut lanes_by_volume: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut groups = Vec::with_capacity(roots.len());
    let mut members: Vec<Vec<usize>> = Vec::new();
    for (root_index, (volume_id, _)) in roots.iter().enumerate() {
        let volume_key = if volume_id.is_empty() {
            vec![0]
        } else {
            let mut key = vec![1];
            key.extend_from_slice(volume_id);
            key
        };
        let next = lanes_by_volume.len();
        let lane = *lanes_by_volume.entry(volume_key).or_insert_with(|| {
            members.push(Vec::new());
            next
        });
        groups.push(lane);
        members[lane].push(root_index);
    }

    let lanes = members
        .iter()
        .map(|root_indexes| {
            let first = roots.get(root_indexes[0]).map(|(_, device)| device);
            let kind = first.map_or(DeviceKind::Unknown, |device| device.kind);
            let source = first.map_or(DeviceKindSource::Unknown, |device| device.source);
            let kind_agreed = root_indexes.iter().all(|&index| {
                roots
                    .get(index)
                    .is_some_and(|(_, device)| device.kind == kind)
            });
            let source_agreed = root_indexes.iter().all(|&index| {
                roots
                    .get(index)
                    .is_some_and(|(_, device)| device.source == source)
            });
            let kind = if kind_agreed {
                kind
            } else {
                DeviceKind::Unknown
            };
            let source = if kind_agreed && source_agreed {
                source
            } else {
                DeviceKindSource::Unknown
            };
            let mut physical_keys: Vec<&[u8]> = root_indexes
                .iter()
                .filter_map(|&index| roots.get(index)?.1.disk_key.as_deref())
                .filter(|key| !key.is_empty())
                .collect();
            physical_keys.sort_unstable();
            physical_keys.dedup();
            let disk_key = match physical_keys.as_slice() {
                [physical] => tagged_key(1, physical),
                _ => {
                    let volume_id = &roots[root_indexes[0]].0;
                    if volume_id.is_empty() {
                        vec![3]
                    } else {
                        tagged_key(2, volume_id)
                    }
                }
            };
            let disk_seat = du_core_contracts::job::DiskSeat::new(kind, disk_key);
            LaneDevice {
                kind,
                source,
                disk_seat,
            }
        })
        .collect();
    (groups, lanes)
}

fn tagged_key(tag: u8, value: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(value.len() + 1);
    key.push(tag);
    key.extend_from_slice(value);
    key
}

#[cfg(test)]
mod device_tests;
