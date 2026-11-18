use super::{
    DescendantShape, Error, Group, GroupMember, Model, group_ref, level,
    level::{Level, Request},
};
use crate::{
    ResultVersion,
    visualization::{geometry::Rect, model_links::validate_model_links, typed_layout::Dispatcher},
};
use du_core_contracts::{
    ItemRef,
    result::{ResultItemKind, ResultVersionRead},
    visualization::{geometry::Geometry, model::ShapeMeta},
};

/// Extends an admitted shape with its immediate children from the same reader.
/// Root rows and aggregate totals remain unchanged. Children below the minimum area
/// merge into one group, or are only counted, exactly as at the root. A branch that
/// cannot be partitioned, or that finds no shape slot left, stays hidden behind its
/// unchanged child count rather than receiving invented geometry.
pub fn add_child_partition(
    model: &mut Model,
    version: &ResultVersion,
    root_shape: usize,
    dispatcher: &Dispatcher,
    max_shapes: usize,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if version.version_id() != &model.result_revision {
        return Err(Error::InvalidSource);
    }
    let parent = parent_shape(model, root_shape)?;
    if version
        .items()
        .get(parent.item.item_no as usize)
        .is_none_or(|item| item.kind != ResultItemKind::Directory)
    {
        return Err(Error::InvalidSource);
    }
    let points = parent.geometry.points();
    if parent.geometry.primitive() != "rect" || points.len() != 2 {
        return Err(Error::InvalidLayout);
    }
    let bounds = Rect {
        left: points[0].x,
        top: points[0].y,
        right: points[1].x,
        bottom: points[1].y,
    };
    // A model without a remaining slot keeps every deeper branch as a hidden count.
    let slots = max_shapes.saturating_sub(model.shape_count());
    if slots == 0 {
        return Ok(());
    }
    let level = match level::build(
        Request {
            version,
            parent: &parent.item,
            basis: &model.basis,
            bounds,
            dispatcher,
            format: &model.strategy.format,
            strategy: Some(&model.strategy.strategy),
            min_area: model.detail.min_area,
            slots,
            // Nested groups need their member rows registered; not enabled yet.
            allow_group: false,
        },
        cancelled,
    ) {
        Ok(level) => level,
        Err(Error::UnavailableAttribution) => return Ok(()),
        Err(error) => return Err(error),
    };
    let added = build_shapes(&parent, bounds, &level, cancelled)?;
    let omitted = added
        .descendants
        .iter()
        .map(|shape| version.children(shape.item.item_no).len())
        .sum::<usize>()
        + level.skipped.len()
        + level.omitted;
    model.detail.unknown_items += level.unattributed.len() as u64;
    model.detail.groups.extend(added.group);
    model.descendants.extend(added.descendants);
    let previous = version.children(parent.item.item_no).len();
    let row = &mut model.rows[parent.row_index];
    row.omitted_children = row
        .omitted_children
        .checked_sub(previous)
        .ok_or(Error::InvalidLayout)?
        .checked_add(omitted)
        .ok_or(Error::InvalidLayout)?;
    Ok(())
}

struct ParentShape {
    item: ItemRef,
    meta: ShapeMeta,
    geometry: Geometry,
    row_index: usize,
}

fn parent_shape(model: &Model, index: usize) -> Result<ParentShape, Error> {
    if let Some(shape) = model.shapes.get(index) {
        let row = model
            .rows
            .get(shape.row_index)
            .ok_or(Error::InvalidLayout)?;
        return Ok(ParentShape {
            item: row.item.clone(),
            meta: shape.meta.clone(),
            geometry: shape.geometry.clone(),
            row_index: shape.row_index,
        });
    }
    let shape = model
        .descendants
        .get(
            index
                .checked_sub(model.shapes.len())
                .ok_or(Error::InvalidLayout)?,
        )
        .ok_or(Error::InvalidLayout)?;
    let root = shape
        .meta
        .root_branch_ref
        .as_ref()
        .ok_or(Error::InvalidLayout)?;
    let row_index = model
        .rows
        .iter()
        .position(|row| &row.item == root)
        .ok_or(Error::InvalidLayout)?;
    Ok(ParentShape {
        item: shape.item.clone(),
        meta: shape.meta.clone(),
        geometry: shape.geometry.clone(),
        row_index,
    })
}

/// Validates all model-local references after every branch has been added.
pub fn finish_child_partitions(
    model: &Model,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Error> {
    let mut metadata = Vec::with_capacity(model.shape_count());
    for meta in model
        .shapes
        .iter()
        .map(|s| &s.meta)
        .chain(model.descendants.iter().map(|s| &s.meta))
        .chain(model.detail.groups.iter().map(|g| &g.meta))
    {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        metadata.push(meta.clone());
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    validate_model_links(&model.result_revision, &metadata, &model.relationships)
        .map_err(Error::Links)?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(())
}

struct Added {
    descendants: Vec<DescendantShape>,
    group: Option<Group>,
}

// A child shape must lie inside its parent's rectangle.
fn contained(geometry: &Geometry, bounds: Rect) -> bool {
    let points = geometry.points();
    geometry.primitive() == "rect"
        && points.len() == 2
        && points[0].x >= bounds.left
        && points[0].y >= bounds.top
        && points[1].x <= bounds.right
        && points[1].y <= bounds.bottom
}

fn build_shapes(
    parent: &ParentShape,
    bounds: Rect,
    level: &Level,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Added, Error> {
    let depth = parent.meta.visible_depth + 1;
    let mut descendants = Vec::with_capacity(level.shapes.len());
    for (row_index, geometry) in &level.shapes {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let source = level.rows.get(*row_index).ok_or(Error::InvalidLayout)?;
        if source.child.item.result_version != parent.item.result_version
            || source.bytes == 0
            || !contained(geometry, bounds)
        {
            return Err(Error::InvalidLayout);
        }
        descendants.push(DescendantShape {
            item: source.child.item.clone(),
            size: source.child.value.clone(),
            meta: ShapeMeta {
                shape_ref: format!("s{:x}-{depth}", source.child.item.item_no),
                parent_shape_ref: Some(parent.meta.shape_ref.clone()),
                visible_depth: depth,
                root_branch_ref: parent.meta.root_branch_ref.clone(),
            },
            geometry: geometry.clone(),
        });
    }
    let group = match &level.group {
        Some(group) => {
            if !contained(&group.geometry, bounds) {
                return Err(Error::InvalidLayout);
            }
            Some(Group {
                parent: parent.item.clone(),
                group_ref: group_ref(&parent.item),
                meta: ShapeMeta {
                    shape_ref: format!("g{:x}-{depth}", parent.item.item_no),
                    parent_shape_ref: Some(parent.meta.shape_ref.clone()),
                    visible_depth: depth,
                    root_branch_ref: parent.meta.root_branch_ref.clone(),
                },
                geometry: group.geometry.clone(),
                members: level.rows[level.retained..level.positive]
                    .iter()
                    .map(|row| GroupMember {
                        item: row.child.item.clone(),
                        size: row.child.value.clone(),
                    })
                    .collect(),
                total: group.total.clone(),
                size_range: group.size_range.clone(),
                slot: 0,
            })
        }
        None => None,
    };
    Ok(Added { descendants, group })
}
