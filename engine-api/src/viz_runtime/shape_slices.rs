//! Type slices on the shapes of folders whose children are not drawn (F06 README 54,
//! #298).
use super::{Admitted, internal};
use crate::{error::EngineError, viz_pages::type_slices};
use du_core_contracts::visualization::{item_kind::VizItemKind, model::VizShape};
use std::collections::HashSet;

/// Gives every folder shape that draws no child shape its slices. A folder whose
/// children are drawn shows them, so it has none; a bar has no children and always
/// gets them. The rows get theirs from the same function.
pub(super) fn add(
    admitted: &Admitted,
    shapes: &mut [VizShape],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), EngineError> {
    let parents: HashSet<String> = shapes
        .iter()
        .filter_map(|shape| shape.parent_shape_ref.clone().flatten())
        .collect();
    let version = admitted.source.version();
    let filtered = admitted.filter.is_some();
    for shape in shapes.iter_mut() {
        let Some(item) = &shape.item_ref else {
            continue;
        };
        let expanded = shape
            .shape_ref
            .as_ref()
            .is_some_and(|key| parents.contains(key));
        if shape.item_kind != Some(VizItemKind::Folder) || expanded {
            continue;
        }
        shape.type_slices = type_slices::folder_slices(
            version,
            item,
            &shape.legend_class_key,
            &admitted.request.size_basis,
            filtered,
            cancelled,
        )
        .map_err(|_| internal())?;
    }
    Ok(())
}
