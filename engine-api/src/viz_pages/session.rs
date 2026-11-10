//! Binds one immutable strategy registry to one visualization model cache.
//!
//! This internal composition does not implement the public `viz.render` wire
//! contract. The caller supplies trusted evidence from the pinned result.
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

use du_analysis_index::{
    IngestSink,
    visualization::{
        projection::{self, Location},
        typed_layout::Dispatcher,
    },
};
use du_core_contracts::{
    ItemRef,
    result::ResultItem,
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
    pub(super) pages: ModelPages,
    dispatcher: Dispatcher,
    pub(super) pinned: Mutex<Option<super::pinned::PinnedModel>>,
}

/// Policy-owned values for one actual child of the pinned folder.
///
/// The caller must use the registered path-comparison and legend policies;
/// display names and raw path bytes do not establish a comparison key.
pub struct ChildPolicyEvidence {
    pub path_compare_key: Vec<u8>,
    pub legend_class_key: String,
    pub display_flags: Vec<String>,
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
            pinned: Mutex::new(None),
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

    /// Renders the pinned folder's actual immediate children and matching table rows.
    ///
    /// The injected policy supplies keys and flags only; item references and
    /// membership are derived from the admitted result. A missing policy must
    /// return an error instead of inventing keys or classifications.
    pub fn render_selected(
        &self,
        request: render::Request<'_>,
        mut evidence: impl FnMut(
            &CurrentAnalysis,
            &ResultItem,
            &mut dyn FnMut() -> bool,
        ) -> Result<ChildPolicyEvidence, render::Error>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<render::Rendered, render::Error> {
        let parent = request.parent.clone();
        self.pages.render_selected(
            &self.dispatcher,
            request,
            |source, cancelled| {
                let folder = du_analysis_index::browse::open(source.version(), Some(&parent))
                    .map_err(|_| render::Error::Projection(projection::Error::InvalidSource))?;
                let mut children = Vec::with_capacity(folder.children.len());
                for item in folder.children {
                    if cancelled() {
                        return Err(render::Error::Cancelled);
                    }
                    let policy = evidence(source, item, cancelled)?;
                    children.push(render::ChildEvidence {
                        item: ItemRef {
                            result_version: source.result_version().clone(),
                            item_no: item.item_no,
                        },
                        path_compare_key: policy.path_compare_key,
                        legend_class_key: policy.legend_class_key,
                        display_flags: policy.display_flags,
                    });
                }
                if cancelled() {
                    return Err(render::Error::Cancelled);
                }
                Ok(children)
            },
            cancelled,
        )
    }
}
