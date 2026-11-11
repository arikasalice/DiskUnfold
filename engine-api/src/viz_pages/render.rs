//! Composes the admitted live single-parent visualization and its page registry.
mod deferred;
pub use deferred::DeferredRender;
use deferred::PrepareMode;

use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use du_analysis_index::visualization::{
    geometry::Rect,
    projection::{self, Options, Source},
    treemap::ChildKey,
    typed_layout::Dispatcher,
};
use du_core_contracts::{
    Id, ItemRef, SizeBasis,
    visualization::{LayoutStrategyKey, render::VizFallback},
};

use super::{
    ModelPages, PreparedModelInput, prepare_scoped_model_input,
    production::{self, RowPresentation},
};
use crate::{current_analysis, viz_catalog};

/// Normalized rendering choices for the exact, unfiltered single-parent path.
pub struct Request<'a> {
    pub selected_revision: &'a Id,
    pub requested_revision: Option<&'a Id>,
    pub parent: &'a ItemRef,
    pub basis: SizeBasis,
    pub bounds: Rect,
    pub format: &'a str,
    pub strategy: Option<&'a LayoutStrategyKey>,
    pub max_shapes: usize,
}

/// Facts supplied by the path-comparison and presentation owners for one item.
pub struct ChildEvidence {
    pub item: ItemRef,
    pub path_compare_key: Option<Vec<u8>>,
    pub legend_class_key: String,
    pub display_flags: Vec<String>,
}

/// Geometry and the model reference use the same pinned source and normalization.
#[derive(Clone)]
pub struct Rendered {
    pub model_ref: String,
    pub model: Arc<projection::Model>,
}

/// Why a committed model has a table but no geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    NoAttribution,
    OrderingUnavailable,
}

/// A successfully committed geometry model or table-only fallback.
#[derive(Clone)]
pub enum CommittedRender {
    Model(Rendered),
    Fallback {
        reason: FallbackReason,
        fallback: VizFallback,
    },
}

impl Rendered {
    /// Returns the table reference for the exact displayed geometry and result revision.
    pub fn fallback(&self) -> VizFallback {
        VizFallback {
            model_ref: self.model_ref.clone(),
            result_revision: self.model.result_revision.clone(),
        }
    }
}

/// A render failure or a table-only result when geometry cannot be attributed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Source(current_analysis::Error),
    Strategy(viz_catalog::Error),
    Projection(projection::Error),
    /// The same pinned rows remain available without geometry.
    NoAttribution(VizFallback),
    /// Equal positive weights need pinned ordering evidence. Table rows remain available.
    OrderingUnavailable(VizFallback),
    Registration(production::Error),
    Cache(super::Error),
    InvalidEvidence,
    Fingerprint,
    Cancelled,
}

/// An unpublished model candidate bound to the source pinned during preparation.
/// Commit revalidates source and registry; the caller must preserve the dispatcher.
pub struct PreparedRender {
    owner: Weak<()>,
    input: PreparedModelInput,
    pub(super) layout: Option<Arc<projection::Model>>,
    fallback_reason: Option<projection::Error>,
}

impl PreparedRender {
    pub(crate) fn bind_condition(&mut self, fingerprint: String) {
        self.input.condition_fingerprint = fingerprint;
    }
}

/// A private candidate with a reference reserved before entering the F24 gate.
/// The reference is neither registered nor returned to a wire consumer here.
pub struct ReservedRender {
    candidate: PreparedRender,
    reference: String,
}

enum Preparation {
    Cached(Rendered),
    Candidate(PreparedRender),
}
impl ModelPages {
    /// Pins the selected live revision, renders via the strategy registry, and registers its rows.
    /// The cancellation callback must be nonblocking and must not reenter the model registry.
    pub fn render_selected(
        &self,
        dispatcher: &Dispatcher,
        request: Request<'_>,
        evidence: impl FnOnce(
            &current_analysis::CurrentAnalysis,
            &mut dyn FnMut() -> bool,
        ) -> Result<Vec<ChildEvidence>, Error>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Rendered, Error> {
        match self.prepare_inner(dispatcher, request, evidence, cancelled, true)? {
            Preparation::Cached(rendered) => Ok(rendered),
            Preparation::Candidate(candidate) => {
                match self.commit_prepared(candidate, cancelled)? {
                    CommittedRender::Model(rendered) => Ok(rendered),
                    CommittedRender::Fallback {
                        reason: FallbackReason::NoAttribution,
                        fallback,
                    } => Err(Error::NoAttribution(fallback)),
                    CommittedRender::Fallback {
                        reason: FallbackReason::OrderingUnavailable,
                        fallback,
                    } => Err(Error::OrderingUnavailable(fallback)),
                }
            }
        }
    }

    /// Computes an unpublished candidate without allocating a model reference or changing the cache.
    /// The evidence must come from the supplied pinned source and trusted policies.
    pub fn prepare_selected(
        &self,
        dispatcher: &Dispatcher,
        request: Request<'_>,
        evidence: impl FnOnce(
            &current_analysis::CurrentAnalysis,
            &mut dyn FnMut() -> bool,
        ) -> Result<Vec<ChildEvidence>, Error>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<PreparedRender, Error> {
        match self.prepare_inner(dispatcher, request, evidence, cancelled, false)? {
            Preparation::Candidate(candidate) => Ok(candidate),
            Preparation::Cached(_) => Err(Error::Cache(super::Error::InvalidModel)),
        }
    }

    fn prepare_inner(
        &self,
        dispatcher: &Dispatcher,
        request: Request<'_>,
        evidence: impl FnOnce(
            &current_analysis::CurrentAnalysis,
            &mut dyn FnMut() -> bool,
        ) -> Result<Vec<ChildEvidence>, Error>,
        cancelled: &mut dyn FnMut() -> bool,
        use_cache: bool,
    ) -> Result<Preparation, Error> {
        check(cancelled)?;
        let source = current_analysis::admit_selected(
            &self.source,
            request.selected_revision,
            request.requested_revision,
        )
        .map_err(Error::Source)?;
        self.prepare_from(
            dispatcher,
            request,
            &source,
            evidence,
            cancelled,
            if use_cache {
                PrepareMode::Cached
            } else {
                PrepareMode::Prepared
            },
        )
    }

    fn prepare_from(
        &self,
        dispatcher: &Dispatcher,
        request: Request<'_>,
        source: &current_analysis::CurrentAnalysis,
        evidence: impl FnOnce(
            &current_analysis::CurrentAnalysis,
            &mut dyn FnMut() -> bool,
        ) -> Result<Vec<ChildEvidence>, Error>,
        cancelled: &mut dyn FnMut() -> bool,
        mode: PrepareMode,
    ) -> Result<Preparation, Error> {
        check(cancelled)?;
        if &request.parent.result_version != source.result_version() {
            return Err(Error::InvalidEvidence);
        }
        let descriptor = viz_catalog::select(dispatcher, request.format, request.strategy)
            .map_err(Error::Strategy)?;
        let evidence = evidence(source, cancelled)?;
        let normalized = normalize(evidence, source.result_version(), cancelled)?;
        let fingerprint = fingerprint(&request, &descriptor.strategy, &normalized, cancelled)?;
        check(cancelled)?;
        if matches!(mode, PrepareMode::Cached)
            && let Some(cached) = self.cached_render(source.version(), &fingerprint, cancelled)?
        {
            // Recheck cancellation after cache-admission locks are released.
            check(cancelled)?;
            return Ok(Preparation::Cached(cached));
        }
        let mut keys = Vec::with_capacity(normalized.len());
        let mut presentations = Vec::with_capacity(normalized.len());
        for child in normalized.into_values() {
            check(cancelled)?;
            let (key, presentation) = split_evidence(child);
            keys.extend(key);
            presentations.push(presentation);
        }
        let project = if matches!(mode, PrepareMode::Hierarchical) {
            projection::project_deferred
        } else {
            projection::project
        };
        let model = project(
            Source {
                version: source.version(),
                parent: request.parent,
                basis: request.basis.clone(),
                bounds: request.bounds,
                keys: &keys,
            },
            dispatcher,
            Options {
                format: request.format,
                strategy: Some(&descriptor.strategy),
                max_shapes: request.max_shapes,
            },
            cancelled,
        );
        let model = match model {
            Ok(model) => model,
            Err(
                reason @ (projection::Error::UnavailableAttribution
                | projection::Error::OrderingUnavailable),
            ) => {
                let input = production::project_fallback(
                    source.version().clone(),
                    request.parent,
                    &request.basis,
                    format!("fallback:{fingerprint}"),
                    presentations,
                    cancelled,
                )
                .map_err(Error::Registration)?;
                check(cancelled)?;
                let input = prepare_scoped_model_input(input, request.parent)
                    .map_err(registration_error)?;
                return Ok(Preparation::Candidate(PreparedRender {
                    owner: Arc::downgrade(&self.identity),
                    input,
                    layout: None,
                    fallback_reason: Some(reason),
                }));
            }
            Err(error) => return Err(Error::Projection(error)),
        };
        let input = production::project_rows(
            source.version().clone(),
            &model,
            fingerprint,
            presentations,
            cancelled,
        )
        .map_err(Error::Registration)?;
        check(cancelled)?;
        let input =
            prepare_scoped_model_input(input, request.parent).map_err(registration_error)?;
        Ok(Preparation::Candidate(PreparedRender {
            owner: Arc::downgrade(&self.identity),
            input,
            layout: Some(Arc::new(model)),
            fallback_reason: None,
        }))
    }

    /// Publishes a prepared candidate only if its exact source is still current.
    ///
    /// Cancellation is checked before the cache mutation. The caller must arbitrate
    /// this commit with job cancellation; this method does not provide an F24 gate.
    /// The cancellation callback must be nonblocking and must not reenter the
    /// source store or model session, including while publication is locked.
    pub fn commit_prepared(
        &self,
        candidate: PreparedRender,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<CommittedRender, Error> {
        check(cancelled)?;
        let reserved = self.reserve_prepared(candidate);
        self.commit_reserved(reserved, cancelled)
    }

    /// Reserves an internal identifier without publishing the candidate.
    /// This can call the injected identity port and must run outside F24 gates.
    pub fn reserve_prepared(&self, candidate: PreparedRender) -> ReservedRender {
        ReservedRender {
            candidate,
            reference: self.ids.next_id().as_str().to_owned(),
        }
    }

    /// Commits with a previously reserved identifier and makes no identity-port call.
    /// The caller must arbitrate this transaction with F24 cancellation.
    pub fn commit_reserved(
        &self,
        reserved: ReservedRender,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<CommittedRender, Error> {
        check(cancelled)?;
        let ReservedRender {
            candidate,
            reference,
        } = reserved;
        let PreparedRender {
            owner,
            input,
            layout,
            fallback_reason,
        } = candidate;
        if !owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &self.identity))
        {
            return Err(Error::Cache(super::Error::InvalidModel));
        }
        use du_core_contracts::result::ResultVersionRead;
        let revision = input.version.version_id().clone();
        let (model_ref, model) = self
            .register_prepared_with_reference(input, layout, cancelled, reference)
            .map_err(registration_error)?;
        if let Some(reason) = fallback_reason {
            let fallback = VizFallback {
                model_ref,
                result_revision: revision,
            };
            return Ok(CommittedRender::Fallback {
                reason: if reason == projection::Error::OrderingUnavailable {
                    FallbackReason::OrderingUnavailable
                } else {
                    FallbackReason::NoAttribution
                },
                fallback,
            });
        }
        let model = model.ok_or(Error::Cache(super::Error::InvalidModel))?;
        Ok(CommittedRender::Model(Rendered { model_ref, model }))
    }

    fn cached_render(
        &self,
        version: &Arc<du_analysis_index::ResultVersion>,
        fingerprint: &str,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Option<Rendered>, Error> {
        use du_core_contracts::result::ResultVersionRead;
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Cache(super::Error::Unavailable))?;
        let key = (version.version_id().clone(), fingerprint.to_owned());
        let Some(model_ref) = state.conditions.get(&key).cloned() else {
            return Ok(None);
        };
        check(cancelled)?;
        self.source
            .with_current(version, || {
                let entry = state
                    .entries
                    .get(&model_ref)
                    .ok_or(Error::Cache(super::Error::Unavailable))?;
                let matches = entry
                    .source
                    .upgrade()
                    .is_some_and(|source| Arc::ptr_eq(&source, version));
                if !matches {
                    return Err(Error::Cache(super::Error::StaleVersion));
                }
                let model = entry
                    .layout
                    .clone()
                    .ok_or(Error::Cache(super::Error::InvalidModel))?;
                super::touch(&mut state.lru, &model_ref);
                Ok(Some(Rendered { model_ref, model }))
            })
            .map_err(|error| Error::Cache(super::source_error(error)))?
    }
}

fn registration_error(error: super::Error) -> Error {
    match error {
        super::Error::Cancelled => Error::Cancelled,
        _ => Error::Registration(production::Error::Pages(error)),
    }
}

fn normalize(
    evidence: Vec<ChildEvidence>,
    revision: &Id,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<BTreeMap<u32, ChildEvidence>, Error> {
    let mut normalized = BTreeMap::new();
    for mut child in evidence {
        check(cancelled)?;
        child.display_flags.sort_unstable();
        child.display_flags.dedup();
        if &child.item.result_version != revision
            || normalized.insert(child.item.item_no, child).is_some()
        {
            return Err(Error::InvalidEvidence);
        }
    }
    Ok(normalized)
}

fn fingerprint(
    request: &Request<'_>,
    strategy: &LayoutStrategyKey,
    evidence: &BTreeMap<u32, ChildEvidence>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<String, Error> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"diskunfold.live-viz.unfiltered.v1\0");
    let bounds = request.bounds;
    let envelope = (
        request.parent,
        &request.basis,
        [bounds.left, bounds.top, bounds.right, bounds.bottom],
        request.format,
        (&strategy.id, strategy.revision),
        request.max_shapes,
    );
    serde_json::to_writer(&mut hash, &envelope).map_err(|_| Error::Fingerprint)?;
    for child in evidence.values() {
        check(cancelled)?;
        hash.update(b"\n");
        let row = (
            &child.item,
            &child.path_compare_key,
            &child.legend_class_key,
            &child.display_flags,
        );
        serde_json::to_writer(&mut hash, &row).map_err(|_| Error::Fingerprint)?;
    }
    Ok(hash.finalize().to_hex().to_string())
}

fn split_evidence(child: ChildEvidence) -> (Option<ChildKey>, RowPresentation) {
    (
        child.path_compare_key.map(|path_compare_key| ChildKey {
            item: child.item.clone(),
            path_compare_key,
        }),
        RowPresentation {
            item: child.item,
            legend_class_key: child.legend_class_key,
            display_flags: child.display_flags,
        },
    )
}

fn check(cancelled: &mut dyn FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
