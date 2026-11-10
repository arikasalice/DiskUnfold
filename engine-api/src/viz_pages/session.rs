//! Binds one immutable strategy registry to one visualization model cache.
//!
//! This internal composition does not implement the public `viz.render` wire
//! contract. The caller supplies trusted evidence from the pinned result.
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

use du_analysis_index::{
    IngestSink, QueryError, ResultVersion,
    visualization::{
        projection::{self, Location},
        typed_layout::Dispatcher,
    },
};
use du_core_contracts::{
    Id, ItemRef,
    result::{ResultItem, ResultVersionRead},
    visualization::rows::{VizRow, VizRows, VizRowsRequest},
};
use du_job_runtime::JobIdentity;

use super::{Error, ModelPages, render, source_error};
use crate::current_analysis::CurrentAnalysis;

/// Owns the registry and model cache for the lifetime of a visualization session.
///
/// Keeping these together prevents a cached model from being reused with a
/// different implementation registered under the same strategy identity.
pub struct Session {
    pages: ModelPages,
    dispatcher: Dispatcher,
    pinned: Mutex<Option<PinnedModel>>,
}

/// Policy-owned values for one actual child of the pinned folder.
///
/// The caller must use the registered path-comparison and legend policies;
/// display names and raw path bytes do not establish a comparison key.
pub struct ChildPolicyEvidence {
    /// Comparison evidence retained for this exact result, if available.
    pub path_compare_key: Option<Vec<u8>>,
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
    /// membership are derived from the admitted result. Missing comparison
    /// evidence is represented by None. Classification still needs its policy.
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

struct PinnedModel {
    reference: String,
    source: Arc<ResultVersion>,
    rows: Arc<[VizRow]>,
    layout: Arc<projection::Model>,
}

impl Session {
    /// Retains exactly one current rendered model until explicitly released.
    /// An existing pin must be released before another can be retained.
    pub fn pin_model(&self, model_ref: &str, revision: &Id) -> Result<(), Error> {
        if model_ref.is_empty() {
            return Err(Error::InvalidRange);
        }
        let mut slot = self.pinned.lock().map_err(|_| Error::Unavailable)?;
        if slot.is_some() {
            return Err(Error::InvalidModel);
        }
        let state = self.pages.state.lock().map_err(|_| Error::Unavailable)?;
        let entry = state.entries.get(model_ref).ok_or(Error::ModelExpired)?;
        if &entry.revision != revision {
            return Err(Error::StaleVersion);
        }
        let source = self
            .pages
            .source
            .pin_current(revision)
            .map_err(source_error)?;
        let registered = entry.source.upgrade().ok_or(Error::StaleVersion)?;
        if !Arc::ptr_eq(&source, &registered) {
            return Err(Error::StaleVersion);
        }
        let layout = entry.layout.clone().ok_or(Error::InvalidModel)?;
        let rows = source.pin_viz_rows(model_ref).map_err(super::model_error)?;
        *slot = Some(PinnedModel {
            reference: model_ref.to_owned(),
            source,
            rows,
            layout,
        });
        Ok(())
    }

    /// Releases the retained model; an already admitted read can finish independently.
    pub fn release_pinned_model(&self, model_ref: &str) -> Result<(), Error> {
        let mut slot = self.pinned.lock().map_err(|_| Error::Unavailable)?;
        if slot.as_ref().is_none_or(|pin| pin.reference != model_ref) {
            return Err(Error::ModelExpired);
        }
        let retired = slot.take();
        drop(slot);
        drop(retired);
        Ok(())
    }

    /// Reads rows from the explicitly retained model, even after publication advances.
    pub fn pinned_rows(&self, request: &VizRowsRequest) -> Result<VizRows, Error> {
        request.validate().map_err(|_| Error::InvalidRange)?;
        let slot = self.pinned.lock().map_err(|_| Error::Unavailable)?;
        let pin = slot
            .as_ref()
            .filter(|pin| pin.reference == request.model_ref)
            .ok_or(Error::ModelExpired)?;
        if pin.source.version_id() != &request.result_revision {
            return Err(Error::StaleVersion);
        }
        let source = pin.source.clone();
        let rows = pin.rows.clone();
        drop(slot);
        let start = request.start as usize;
        if start > rows.len() {
            return Err(Error::InvalidRange);
        }
        let end = start.saturating_add(request.count as usize).min(rows.len());
        Ok(VizRows {
            rows: rows[start..end].to_vec(),
            total_count: rows.len() as u64,
            result_revision: source.version_id().clone(),
            observed_at: source.observed_at().cloned(),
            completeness: source.completeness(),
        })
    }

    /// Locates an item in the exact retained source and layout.
    pub fn pinned_locate(&self, model_ref: &str, item: &ItemRef) -> Result<Location, Error> {
        let slot = self.pinned.lock().map_err(|_| Error::Unavailable)?;
        let pin = slot
            .as_ref()
            .filter(|pin| pin.reference == model_ref)
            .ok_or(Error::ModelExpired)?;
        if pin.source.version_id() != &item.result_version {
            return Err(Error::StaleVersion);
        }
        let source = pin.source.clone();
        let layout = pin.layout.clone();
        drop(slot);
        layout.locate(&source, item).map_err(|error| match error {
            QueryError::NotFound => Error::ItemUnavailable,
            QueryError::StaleVersion => Error::StaleVersion,
            _ => Error::Unavailable,
        })
    }
}
