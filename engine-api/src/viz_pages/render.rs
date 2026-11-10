//! Composes the admitted live single-parent visualization and its page registry.
use std::{collections::BTreeMap, sync::Arc};

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
    ModelPages,
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
    pub path_compare_key: Vec<u8>,
    pub legend_class_key: String,
    pub display_flags: Vec<String>,
}

/// Geometry and the model reference use the same pinned source and normalization.
pub struct Rendered {
    pub model_ref: String,
    pub model: Arc<projection::Model>,
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

/// A failed render never registers a completed model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Source(current_analysis::Error),
    Strategy(viz_catalog::Error),
    Projection(projection::Error),
    Registration(production::Error),
    Cache(super::Error),
    InvalidEvidence,
    Fingerprint,
    Cancelled,
}

impl ModelPages {
    /// Pins the selected live revision, renders via the strategy registry, and registers its rows.
    /// The evidence callback must use the supplied pinned source and trusted in-process policies.
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
        check(cancelled)?;
        let source = current_analysis::admit_selected(
            &self.source,
            request.selected_revision,
            request.requested_revision,
        )
        .map_err(Error::Source)?;
        if &request.parent.result_version != source.result_version() {
            return Err(Error::InvalidEvidence);
        }
        let descriptor = viz_catalog::select(dispatcher, request.format, request.strategy)
            .map_err(Error::Strategy)?;
        let evidence = evidence(&source, cancelled)?;
        let normalized = normalize(evidence, source.result_version(), cancelled)?;
        let fingerprint = fingerprint(&request, &descriptor.strategy, &normalized, cancelled)?;
        check(cancelled)?;
        if let Some(cached) = self.cached_render(source.version(), &fingerprint)? {
            return Ok(cached);
        }
        let mut keys = Vec::with_capacity(normalized.len());
        let mut presentations = Vec::with_capacity(normalized.len());
        for child in normalized.into_values() {
            check(cancelled)?;
            let (key, presentation) = split_evidence(child);
            keys.push(key);
            presentations.push(presentation);
        }
        let model = projection::project(
            Source {
                version: source.version(),
                parent: request.parent,
                basis: request.basis,
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
        )
        .map_err(Error::Projection)?;
        let input = production::project_rows(
            source.version().clone(),
            &model,
            fingerprint,
            presentations,
            cancelled,
        )
        .map_err(Error::Registration)?;
        check(cancelled)?;
        let (model_ref, model) = self
            .register_with_layout(input, Some(Arc::new(model)))
            .map_err(|error| Error::Registration(production::Error::Pages(error)))?;
        let model = model.ok_or(Error::Cache(super::Error::InvalidModel))?;
        Ok(Rendered { model_ref, model })
    }

    fn cached_render(
        &self,
        version: &Arc<du_analysis_index::ResultVersion>,
        fingerprint: &str,
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
        let current = self
            .source
            .pin_current(version.version_id())
            .map_err(|error| Error::Cache(super::source_error(error)))?;
        let entry = state
            .entries
            .get(&model_ref)
            .ok_or(Error::Cache(super::Error::Unavailable))?;
        let matches = Arc::ptr_eq(version, &current)
            && entry
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

fn split_evidence(child: ChildEvidence) -> (ChildKey, RowPresentation) {
    (
        ChildKey {
            item: child.item.clone(),
            path_compare_key: child.path_compare_key,
        },
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
