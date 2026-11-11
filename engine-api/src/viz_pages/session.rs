//! Binds one immutable strategy registry to one visualization model cache.
//!
//! This internal composition does not implement the public `viz.render` wire
//! contract. The caller supplies trusted evidence from the pinned result.
pub(crate) mod expansion;
pub(crate) use expansion::Policy as ExpansionPolicy;

use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

use du_analysis_index::{
    IngestSink, ResultVersion,
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
    /// Computes histogram input through the same frozen registry as other layouts.
    pub fn compute_histogram(
        &self,
        source: Arc<ResultVersion>,
        request: du_analysis_index::visualization::histogram_dispatch::Request<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<super::histogram::PinnedRendered, super::histogram::Error> {
        super::histogram::PinnedRendered::compute(source, request, &self.dispatcher, cancelled)
    }

    /// Computes a histogram through the exact descriptor admitted by this session.
    pub(crate) fn compute_histogram_selected(
        &self,
        source: Arc<ResultVersion>,
        request: du_analysis_index::visualization::histogram_dispatch::Request<'_>,
        descriptor: &du_core_contracts::visualization::LayoutDescriptor,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<super::histogram::PinnedRendered, super::histogram::Error> {
        super::histogram::PinnedRendered::compute_selected(
            source,
            request,
            &self.dispatcher,
            descriptor,
            cancelled,
        )
    }

    /// Reserves histogram rows and metadata for a caller-owned publication gate.
    pub fn defer_histogram(
        &self,
        candidate: super::histogram::SelectedCandidate,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<super::histogram::DeferredSelectedPages, super::histogram::Error> {
        self.pages.defer_histogram_pages(candidate, cancelled)
    }

    /// Commits the reserved histogram while the caller owns its publication gate.
    pub fn commit_histogram(
        &self,
        candidate: &mut super::histogram::DeferredSelectedPages,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<String, Error> {
        self.pages.commit_histogram_pages(candidate, cancelled)
    }

    /// Reads the committed histogram's table and geometry status.
    pub fn histogram_frame(
        &self,
        reference: &str,
    ) -> Result<Arc<super::histogram::HistogramFrame>, Error> {
        self.pages.histogram_frame(reference)
    }

    /// Reads members from a group in the same committed histogram model.
    pub fn histogram_group_rows(
        &self,
        reference: &str,
        group: &str,
        start: u32,
        count: u32,
    ) -> Result<super::GroupPage, Error> {
        self.pages.group_rows(reference, group, start, count)
    }
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
        evidence: impl FnMut(
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
            |source, cancelled| collect_evidence(source, &parent, evidence, cancelled),
            cancelled,
        )
    }

    /// Prepares an unpublished model using this session's frozen dispatcher.
    ///
    /// Membership and item references come from the admitted source; the policy
    /// supplies only comparison and presentation evidence for each actual child.
    /// No model reference is allocated and no rows are registered by this call.
    pub fn prepare_selected(
        &self,
        request: render::Request<'_>,
        evidence: impl FnMut(
            &CurrentAnalysis,
            &ResultItem,
            &mut dyn FnMut() -> bool,
        ) -> Result<ChildPolicyEvidence, render::Error>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<render::PreparedRender, render::Error> {
        let parent = request.parent.clone();
        self.pages.prepare_selected(
            &self.dispatcher,
            request,
            |source, cancelled| collect_evidence(source, &parent, evidence, cancelled),
            cancelled,
        )
    }

    /// Reserves a private reference before entering the runtime publication gate.
    pub fn reserve_prepared(&self, candidate: render::PreparedRender) -> render::ReservedRender {
        self.pages.reserve_prepared(candidate)
    }

    /// Allocates deferred-destruction ownership before entering the F24 gate.
    pub fn defer_reserved(&self, candidate: render::ReservedRender) -> render::DeferredRender {
        self.pages.defer_reserved(candidate)
    }

    /// Attempts a single commit while retaining discarded allocations in the caller.
    /// The caller must keep `candidate` alive until after the F24 gate is released.
    pub fn commit_deferred(
        &self,
        candidate: &mut render::DeferredRender,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<render::CommittedRender, render::Error> {
        self.pages.commit_deferred(candidate, cancelled)
    }

    /// Publishes a reserved candidate without calling the injected identity port.
    /// The caller must serialize this commit against cancellation and must not
    /// reenter the model or source stores from the cancellation check.
    pub fn commit_reserved(
        &self,
        candidate: render::ReservedRender,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<render::CommittedRender, render::Error> {
        self.pages.commit_reserved(candidate, cancelled)
    }

    /// Commits a candidate prepared by this session if its exact source is current.
    ///
    /// Both geometry and a table-only fallback are successful commits. The
    /// callback must not block or reenter the source store or model session.
    /// The caller must arbitrate publication with job cancellation separately.
    pub fn commit_prepared(
        &self,
        candidate: render::PreparedRender,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<render::CommittedRender, render::Error> {
        self.pages.commit_prepared(candidate, cancelled)
    }
}

fn collect_evidence(
    source: &CurrentAnalysis,
    parent: &ItemRef,
    mut evidence: impl FnMut(
        &CurrentAnalysis,
        &ResultItem,
        &mut dyn FnMut() -> bool,
    ) -> Result<ChildPolicyEvidence, render::Error>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<render::ChildEvidence>, render::Error> {
    let folder = du_analysis_index::browse::open(source.version(), Some(parent))
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
}

struct PinnedModel {
    reference: String,
    source: Arc<ResultVersion>,
    rows: Arc<[VizRow]>,
    layout: Option<Arc<projection::Model>>,
    scope: Option<ItemRef>,
}

impl Session {
    /// Retains exactly one current rendered or table-only model until explicitly released.
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
        let layout = entry.layout.clone();
        let scope = entry.scope.clone();
        let rows = source.pin_viz_rows(model_ref).map_err(super::model_error)?;
        *slot = Some(PinnedModel {
            reference: model_ref.to_owned(),
            source,
            rows,
            layout,
            scope,
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

    /// Locates an item in the exact retained source and its recorded scope.
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
        let scope = pin.scope.clone();
        drop(slot);
        super::locate::locate(&source, layout.as_deref(), scope.as_ref(), item)
    }
}
impl Session {
    /// Resolves a strategy using this session's immutable registry.
    pub(crate) fn select_strategy(
        &self,
        format: &str,
        key: Option<&du_core_contracts::visualization::LayoutStrategyKey>,
    ) -> Result<du_core_contracts::visualization::LayoutDescriptor, crate::viz_catalog::Error> {
        crate::viz_catalog::select(&self.dispatcher, format, key)
    }

    pub(crate) fn prepare_pinned(
        &self,
        request: render::Request<'_>,
        source: &CurrentAnalysis,
        defer_links: bool,
        evidence: impl FnMut(
            &CurrentAnalysis,
            &ResultItem,
            &mut dyn FnMut() -> bool,
        ) -> Result<ChildPolicyEvidence, render::Error>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<render::PreparedRender, render::Error> {
        let parent = request.parent.clone();
        self.pages.prepare_pinned(
            &self.dispatcher,
            request,
            source,
            defer_links,
            |source, cancelled| collect_evidence(source, &parent, evidence, cancelled),
            cancelled,
        )
    }
}

impl Session {
    /// Adds depth-two geometry before publication; direct-child page rows are untouched.
    pub(crate) fn extend_pinned(
        &self,
        candidate: &mut render::PreparedRender,
        source: &CurrentAnalysis,
        policy: ExpansionPolicy<'_>,
        mut evidence: impl FnMut(
            &CurrentAnalysis,
            &ResultItem,
            &mut dyn FnMut() -> bool,
        ) -> Result<ChildPolicyEvidence, render::Error>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), render::Error> {
        let Some(layout) = candidate.layout.as_mut() else {
            return Ok(());
        };
        expansion::extend(
            Arc::make_mut(layout),
            source,
            &self.dispatcher,
            policy,
            &mut evidence,
            cancelled,
        )
    }
}
