//! Bind visualization to the same source and worker pool as desktop analysis.
use std::{num::NonZeroUsize, sync::Arc};

use du_analysis_index::{IngestSink, visualization::bundle};
use du_core_contracts::ports::Random;
use du_job_runtime::JobIdentity;

use crate::{
    api::EngineBuilder,
    viz_pages::session::Session,
    viz_runtime::{
        Assembly, EvidenceProducer, Runtime, ViewPolicy, selection::WindowScanSelection,
    },
    workers::Workers,
};

/// Shares desktop-owned resources with opt-in visualization composition.
pub struct VisualizationServices {
    source: IngestSink,
    ids: Arc<dyn JobIdentity>,
    workers: Arc<Workers>,
}

/// Evidence and limits for the partial development registry.
pub struct VisualizationPreview {
    pub producer: Arc<dyn EvidenceProducer>,
    pub selection: Arc<dyn WindowScanSelection>,
    pub policy: ViewPolicy,
    pub model_capacity: NonZeroUsize,
}

impl VisualizationServices {
    pub(super) fn new(
        source: IngestSink,
        ids: Arc<dyn JobIdentity>,
        workers: Arc<Workers>,
    ) -> Self {
        Self {
            source,
            ids,
            workers,
        }
    }

    /// Registers the available treemap for explicit development and integration tests.
    ///
    /// The window must supply its selected revision and its own evidence. This
    /// registry is partial and cannot serve as the default product catalog.
    pub fn register_preview(
        &self,
        builder: &mut EngineBuilder,
        random: &dyn Random,
        options: VisualizationPreview,
    ) -> Result<(), &'static str> {
        let dispatcher = bundle::implemented_dispatcher()
            .map_err(|_| "visualization preview strategy registration failed")?;
        let session = Arc::new(Session::new(
            self.source.clone(),
            options.model_capacity,
            self.ids.clone(),
            dispatcher,
        ));
        Runtime::register_on_desktop(
            builder,
            Assembly {
                source: self.source.clone(),
                session,
                jobs: self.workers.runtime.clone(),
                ids: self.ids.clone(),
                producer: options.producer,
                policy: options.policy,
            },
            random,
            options.selection,
            self.workers.clone(),
        )?;
        Ok(())
    }
}
