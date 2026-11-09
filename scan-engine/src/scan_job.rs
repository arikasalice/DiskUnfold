//! Job-runtime adapter for collecting an already verified root.
use std::sync::{Arc, Mutex};

use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    job::{
        HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobOutcome, ProgressUnit,
        ResourceClass,
    },
    scan_observation::AnchoredDirectory,
};

use crate::scan::{ResultIds, collect};

const KIND: &str = "scan.metadata";

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
    root: Mutex<Option<Box<dyn AnchoredDirectory>>>,
    store: IngestSink,
    scope: Id,
    ids: Arc<dyn ResultIds>,
}

impl MetadataScan {
    pub fn new(
        root: Box<dyn AnchoredDirectory>,
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
            Some(root) => collect(
                &self.store,
                self.scope.clone(),
                root,
                &*self.ids,
                || context.checkpoint(),
                |_| true,
            ),
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
