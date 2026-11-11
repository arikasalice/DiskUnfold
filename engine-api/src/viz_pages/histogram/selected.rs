//! Bind registered histogram geometry to same-source page preparation.
use super::{Candidate, Error, Request, assemble};
use du_analysis_index::{
    ResultVersion,
    visualization::{
        geometry::Rect, histogram_dispatch, histogram_projection, typed_layout::Dispatcher,
    },
};
use du_core_contracts::result::ResultVersionRead;
use std::sync::Arc;

/// Immutable selected geometry and page evidence from one calculation.
pub struct SelectedCandidate(pub(super) Candidate);

impl SelectedCandidate {
    /// Returns the geometry and source aggregate without exposing mutation.
    pub fn model(&self) -> &histogram_projection::Model {
        &self.0.model
    }
    /// Returns the matching rows and model-local groups without exposing mutation.
    pub fn pages(&self) -> &crate::viz_pages::ModelInput {
        &self.0.pages
    }
}

/// A registered calculation coupled to its exact pinned result allocation.
pub struct PinnedRendered {
    source: Arc<ResultVersion>,
    rendered: histogram_dispatch::Rendered,
}

impl PinnedRendered {
    /// Runs the registered layout after binding its input to the supplied source.
    pub fn compute(
        source: Arc<ResultVersion>,
        request: histogram_dispatch::Request<'_>,
        dispatcher: &Dispatcher,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Self, Error> {
        if !std::ptr::eq(Arc::as_ptr(&source), request.version) {
            return Err(Error::Source);
        }
        let prepared = histogram_dispatch::prepare(request, cancelled).map_err(|e| {
            if e == histogram_dispatch::Error::Cancelled {
                Error::Cancelled
            } else {
                Error::Source
            }
        })?;
        let rendered = prepared.render(dispatcher, cancelled).map_err(|e| {
            if matches!(
                e,
                du_analysis_index::visualization::typed_layout::DispatchError::Layout(
                    du_analysis_index::visualization::typed_layout::Error::Cancelled
                )
            ) {
                Error::Cancelled
            } else {
                Error::Source
            }
        })?;
        Ok(Self { source, rendered })
    }

    /// Returns the exact registered calculation and its retained source evidence.
    pub fn rendered(&self) -> &histogram_dispatch::Rendered {
        &self.rendered
    }
}

/// Uses a selected registered histogram and owner-supplied presentation evidence.
///
/// Source revision, scope, interval table, basis, viewport and scale must match
/// the immutable rendered input. No model or page is registered by this function.
pub fn prepare_selected(
    mut request: Request<'_>,
    pinned: PinnedRendered,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<SelectedCandidate, Error> {
    validate_request(&request, &pinned, cancelled)?;
    bind_table_key(&mut request, &pinned);
    let (unit_bins, model) = materialize_geometry(pinned, cancelled)?;
    let mut candidate = assemble(request, model, cancelled)?;
    candidate.unit_bins = Some(unit_bins);
    Ok(SelectedCandidate(candidate))
}

fn validate_request(
    request: &Request<'_>,
    pinned: &PinnedRendered,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if !Arc::ptr_eq(&request.version, &pinned.source) {
        return Err(Error::Source);
    }
    let rendered = &pinned.rendered;
    let prepared = rendered.prepared();
    let source = prepared.model();
    let selected = rendered.selected();
    if request.fingerprint.is_empty()
        || request.unknown_reason.trim().is_empty()
        || request.bins.len() != request.bin_presentation.len()
        || source.summary.result_version != *request.version.version_id()
        || source.summary.scope != *request.scope
        || prepared.size_basis() != &request.size_basis
        || prepared.unit_bins().bins != request.bins
        || prepared.input().bounds != request.bounds
        || prepared.input().scale != request.scale
        || selected.descriptor != histogram_dispatch::histogram_descriptor()
    {
        return Err(Error::Source);
    }
    for row in &prepared.input().rows {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if let du_core_contracts::visualization::quantity::VizAmount::Unknown { reason_key } =
            row.quantity.amount()
            && reason_key != &request.unknown_reason
        {
            return Err(Error::Source);
        }
    }
    Ok(())
}

fn bind_table_key(request: &mut Request<'_>, pinned: &PinnedRendered) {
    let prepared = pinned.rendered.prepared();
    // Keep the table identity in the private page-cache key even when two
    // revisions happen to produce the same interval values.
    let mut key = blake3::Hasher::new();
    key.update(b"diskunfold.histogram-unit-table.v1");
    key.update(&(request.fingerprint.len() as u64).to_le_bytes());
    key.update(request.fingerprint.as_bytes());
    key.update(&prepared.unit_bins().table_revision.to_le_bytes());
    key.update(&prepared.unit_bins().base.to_le_bytes());
    request.fingerprint = key.finalize().to_hex().to_string();
}

fn materialize_geometry(
    pinned: PinnedRendered,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<
    (
        du_analysis_index::visualization::unit_bins::UnitBins,
        histogram_projection::Model,
    ),
    Error,
> {
    let (unit_bins, _size_basis, mut model, selected) = pinned.rendered.into_parts();
    model.histogram.axis_max = selected.axis_max;
    let mut seen = vec![false; model.histogram.bins.len()];
    for shape in &selected.output.shapes {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let row = model
            .histogram
            .bins
            .get_mut(shape.row_index)
            .ok_or(Error::Source)?;
        let used = seen.get_mut(shape.row_index).ok_or(Error::Source)?;
        if *used || shape.geometry.primitive() != "rect" {
            return Err(Error::Source);
        }
        let [a, b] = shape.geometry.points() else {
            return Err(Error::Source);
        };
        row.rect = Some(Rect {
            left: a.x,
            top: a.y,
            right: b.x,
            bottom: b.y,
        });
        *used = true;
    }
    for omission in &selected.output.skipped {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let row = model
            .histogram
            .bins
            .get_mut(omission.row_index)
            .ok_or(Error::Source)?;
        let used = seen.get_mut(omission.row_index).ok_or(Error::Source)?;
        if *used {
            return Err(Error::Source);
        }
        row.no_area = Some(omission.reason);
        *used = true;
    }
    if seen.iter().any(|used| !used) {
        return Err(Error::Source);
    }
    Ok((unit_bins, model))
}
