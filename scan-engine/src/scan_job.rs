//! Job-runtime adapter for collecting an already verified root.
use std::sync::{Arc, Mutex};

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

use crate::scan::{ResultIds, collect_open_build};

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
    root: Mutex<Option<OpenedScopeRoot>>,
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
        Self {
            root: Mutex::new(Some(root)),
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
        let root = self.root.lock().ok().and_then(|mut root| root.take());
        match root {
            Some(root) => {
                let observed = ResultScopeRoot {
                    path: ResultNativePath {
                        encoding: root.canonical_path.encoding,
                        raw: root.canonical_path.raw,
                    },
                    volume_id: root.volume_id,
                    file_id: root.file_id,
                };
                let build = self.store.open_observed(self.scope.clone(), observed);
                collect_open_build(
                    build,
                    root.directory,
                    &*self.ids,
                    &*context,
                    DEFAULT_SCAN_WORKERS,
                    &|_| true,
                )
            }
            None => JobOutcome {
                counts: Default::default(),
                completed_scope: false,
                failure_reason_key: Some("support.internal_error".into()),
                unprocessed_summary: None,
                result_version_id: None,
            },
        }
    }
}
