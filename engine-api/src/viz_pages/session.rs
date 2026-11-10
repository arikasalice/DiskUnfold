//! Binds one immutable strategy registry to one visualization model cache.
//!
//! This internal composition does not implement the public `viz.render` wire
//! contract. The caller supplies trusted evidence from the pinned result.
use std::{num::NonZeroUsize, sync::Arc};

use du_analysis_index::{
    IngestSink,
    visualization::{projection::Location, typed_layout::Dispatcher},
};
use du_core_contracts::{
    ItemRef,
    visualization::rows::{VizRows, VizRowsRequest},
};
use du_job_runtime::JobIdentity;

use super::{ModelPages, render};
use crate::current_analysis::CurrentAnalysis;

/// Owns the registry and model cache for the lifetime of a visualization session.
///
/// Keeping these together prevents a cached model from being reused with a
/// different implementation registered under the same strategy identity.
pub struct Session {
    pages: ModelPages,
    dispatcher: Dispatcher,
}

impl Session {
    /// Creates a session from a frozen registry and an empty model store.
    pub fn new(
        source: IngestSink,
        capacity: NonZeroUsize,
        ids: Arc<dyn JobIdentity>,
        dispatcher: Dispatcher,
    ) -> Self {
        Self {
            pages: ModelPages::new(source, capacity, ids),
            dispatcher,
        }
    }

    /// Reads a page from the same pinned model as its rendered geometry.
    pub fn rows(&self, request: &VizRowsRequest) -> Result<VizRows, super::Error> {
        self.pages.rows(request)
    }

    /// Resolves an item reference against the same immutable model used for rows.
    pub fn locate(&self, model_ref: &str, item: &ItemRef) -> Result<Location, super::Error> {
        self.pages.locate(model_ref, item)
    }

    /// Renders against the session's registry and pins the matching table rows.
    pub fn render_selected(
        &self,
        request: render::Request<'_>,
        evidence: impl FnOnce(
            &CurrentAnalysis,
            &mut dyn FnMut() -> bool,
        ) -> Result<Vec<render::ChildEvidence>, render::Error>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<render::Rendered, render::Error> {
        self.pages
            .render_selected(&self.dispatcher, request, evidence, cancelled)
    }
}
