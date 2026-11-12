//! Bind visualization to the same source and worker pool as desktop analysis.
use std::{num::NonZeroUsize, sync::Arc};

use du_analysis_index::{
    IngestSink,
    visualization::{bundle, typed_layout::Dispatcher},
};
use du_core_contracts::ports::Random;
use du_job_runtime::JobIdentity;

use crate::{
    api::EngineBuilder,
    viz_catalog,
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
        self.register_preview_runtime(builder, random, options, dispatcher, None)
    }

    /// Registers the preview and its catalog against the same frozen layout registry.
    ///
    /// Catalog criteria must be supplied by the policy owners for this preview.
    pub fn register_preview_with_catalog(
        &self,
        builder: &mut EngineBuilder,
        random: &dyn Random,
        options: VisualizationPreview,
        catalog: viz_catalog::Policy,
    ) -> Result<(), &'static str> {
        let dispatcher = bundle::implemented_dispatcher()
            .map_err(|_| "visualization preview strategy registration failed")?;
        let catalog = viz_catalog::prepare(&dispatcher, catalog)
            .map_err(|_| "visualization preview catalog registration failed")?;
        self.register_preview_runtime(builder, random, options, dispatcher, None)?;
        builder.register(catalog);
        Ok(())
    }

    /// Registers a preview with its GUI availability query and shared policy catalog.
    ///
    /// Availability is opt-in: the preview's `EvidenceProducer` must opt in and
    /// answer both availability and render evidence for the same pinned result.
    /// Render evidence may apply additional basis-specific restrictions. The
    /// dispatcher and required-data table revisions are shared with `viz.catalog`.
    pub fn register_preview_with_catalog_and_availability(
        &self,
        builder: &mut EngineBuilder,
        random: &dyn Random,
        options: VisualizationPreview,
        catalog_policy: viz_catalog::Policy,
    ) -> Result<(), &'static str> {
        let dispatcher = bundle::implemented_dispatcher()
            .map_err(|_| "visualization preview strategy registration failed")?;
        let catalog = viz_catalog::prepare(&dispatcher, catalog_policy)
            .map_err(|_| "visualization preview catalog registration failed")?;
        let producer = options.producer.clone();
        let availability = crate::viz_availability::prepare(
            self.source.clone(),
            catalog.formats().to_vec(),
            catalog.required_data_table_revision(),
            producer,
        )
        .map_err(|error| match error {
            crate::viz_availability::RegistrationError::EvidenceNotSupported => {
                "visualization producer does not support availability"
            }
            _ => "visualization availability registration failed",
        })?;
        self.register_preview_runtime(builder, random, options, dispatcher, Some(availability))?;
        builder.register(catalog);
        Ok(())
    }

    fn register_preview_runtime(
        &self,
        builder: &mut EngineBuilder,
        random: &dyn Random,
        options: VisualizationPreview,
        dispatcher: Dispatcher,
        availability: Option<crate::viz_availability::ReadAvailability>,
    ) -> Result<(), &'static str> {
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
        if let Some(availability) = availability {
            builder.register(availability);
        }
        Ok(())
    }
}
