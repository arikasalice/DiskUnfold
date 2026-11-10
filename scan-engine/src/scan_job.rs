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

use crate::scan::{DevicePlan, ResultIds, collect_open_forest_with_groups};

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
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::MetadataIo],
        history: HistoryMode::Summary,
    }
}

/// Collects from the handle established during scan admission.
pub struct MetadataScan {
    roots: Mutex<Option<DeviceRoots>>,
    store: IngestSink,
    scope: Id,
    ids: Arc<dyn ResultIds>,
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
        Self {
            roots: Mutex::new(Some(roots)),
            store,
            scope,
            ids,
        }
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
                let build = self.store.open_forest(self.scope.clone(), observed);
                collect_open_forest_with_groups(
                    build,
                    directories,
                    &*self.ids,
                    &*context,
                    DEFAULT_SCAN_WORKERS,
                    &|_| true,
                    DevicePlan { groups, limit: 4 },
                )
            }
            _ => JobOutcome {
                counts: Default::default(),
                completed_scope: false,
                failure_reason_key: Some("support.internal_error".into()),
                unprocessed_summary: None,
                result_version_id: None,
            },
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
