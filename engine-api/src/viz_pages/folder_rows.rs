//! Reading the table of an expanded folder by the shape reference of the folder.
//!
//! The reference is model-local: it must name a shape of the same pinned model. A
//! reference the model does not have is not found; a shape it has that is no expanded
//! folder (a file, a group, a folder that was not opened) is an invalid argument.
use super::{Error, model_error};
use du_analysis_index::{ResultVersion, visualization::projection};
use du_core_contracts::visualization::rows::{VizRow, VizRowsRequest};
use std::sync::Arc;

/// Pins the rows a request reads: the root table of the model, or the table of the
/// expanded folder the request names.
pub(super) fn read(
    source: &ResultVersion,
    request: &VizRowsRequest,
    layout: Option<&projection::Model>,
) -> Result<Arc<[VizRow]>, Error> {
    match request.parent_shape_ref.as_deref() {
        None => source.pin_viz_rows(&request.model_ref).map_err(model_error),
        Some(shape) => table(source, &request.model_ref, layout, shape),
    }
}

/// Pins the rows of the expanded folder whose shape reference is `shape`.
///
/// `layout` is the geometry of the model; a table-only model has none and so has no
/// shapes. Only a treemap publishes shape references, so the geometry of any other
/// format has none that a client could name. The table is owned by the source result
/// like the root rows, and is pinned the same way.
pub(super) fn table(
    source: &ResultVersion,
    model_ref: &str,
    layout: Option<&projection::Model>,
    shape: &str,
) -> Result<Arc<[VizRow]>, Error> {
    let layout = layout.filter(|layout| layout.strategy.format == "treemap");
    if !layout.is_some_and(|layout| has_shape(layout, shape)) {
        return Err(Error::ItemUnavailable);
    }
    source
        .pin_viz_table(model_ref, shape)
        .map_err(model_error)?
        .ok_or(Error::NotExpanded)
}

fn has_shape(layout: &projection::Model, shape: &str) -> bool {
    layout
        .shapes
        .iter()
        .map(|shape| &shape.meta)
        .chain(layout.descendants.iter().map(|shape| &shape.meta))
        .chain(layout.detail.groups.iter().map(|group| &group.meta))
        .any(|meta| meta.shape_ref == shape)
}
