//! Structural validation for references in a published visualization model.

use du_core_contracts::{
    Id,
    visualization::model::{ShapeMeta, VizRelationship},
};
use std::collections::HashMap;

/// A broken reference or hierarchy in a proposed model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelLinkError {
    InvalidShapeRef,
    DuplicateShapeRef,
    BranchVersionMismatch,
    MissingParent,
    InvalidParentDepth,
    InvalidRelationshipKind,
    MissingEndpoint,
}

/// Validates all model-local links before an immutable model can be exposed.
///
/// Strictly decreasing depth along parent links prevents self-parenting and
/// cycles, while allowing a rendered shape to skip omitted ancestors.
/// Strategy and geometry keys remain open to preserve forward compatibility.
pub fn validate_model_links(
    result_revision: &Id,
    shapes: &[ShapeMeta],
    relationships: &[VizRelationship],
) -> Result<(), ModelLinkError> {
    let depths = shape_depths(result_revision, shapes)?;
    for shape in shapes {
        if let Some(parent) = &shape.parent_shape_ref {
            let depth = depths
                .get(parent.as_str())
                .ok_or(ModelLinkError::MissingParent)?;
            if *depth >= shape.visible_depth {
                return Err(ModelLinkError::InvalidParentDepth);
            }
        }
    }
    for relationship in relationships {
        if relationship.kind.trim().is_empty() {
            return Err(ModelLinkError::InvalidRelationshipKind);
        }
        if !depths.contains_key(relationship.from_shape_ref.as_str())
            || !depths.contains_key(relationship.to_shape_ref.as_str())
        {
            return Err(ModelLinkError::MissingEndpoint);
        }
    }
    Ok(())
}

fn shape_depths<'a>(
    result_revision: &Id,
    shapes: &'a [ShapeMeta],
) -> Result<HashMap<&'a str, u32>, ModelLinkError> {
    let mut depths = HashMap::with_capacity(shapes.len());
    for shape in shapes {
        if shape.shape_ref.trim().is_empty() {
            return Err(ModelLinkError::InvalidShapeRef);
        }
        if depths
            .insert(shape.shape_ref.as_str(), shape.visible_depth)
            .is_some()
        {
            return Err(ModelLinkError::DuplicateShapeRef);
        }
        if shape
            .root_branch_ref
            .as_ref()
            .is_some_and(|item| &item.result_version != result_revision)
        {
            return Err(ModelLinkError::BranchVersionMismatch);
        }
    }
    Ok(depths)
}
