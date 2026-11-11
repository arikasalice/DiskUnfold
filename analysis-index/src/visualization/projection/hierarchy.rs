use super::*;

/// Extends an admitted shape with its immediate children from the same reader.
/// Root rows and aggregate totals remain unchanged. A branch without attribution
/// or ordering evidence stays omitted rather than receiving invented geometry.
pub fn add_child_partition(
    model: &mut Model,
    version: &ResultVersion,
    root_shape: usize,
    keys: &[ChildKey],
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
    let input = match Input::admit_cancellable(
        version,
        &parent.item,
        model.basis.clone(),
        bounds,
        keys,
        cancelled,
    ) {
        Ok(input) => input,
        Err(
            crate::visualization::treemap::AdmissionError::NoAttribution
            | crate::visualization::treemap::AdmissionError::OrderingUnavailable,
        ) => return Ok(()),
        Err(crate::visualization::treemap::AdmissionError::Cancelled) => {
            return Err(Error::Cancelled);
        }
        Err(_) => return Err(Error::InvalidSource),
    };
    let selected = dispatcher
        .execute(
            &model.strategy.format,
            Some(&model.strategy.strategy),
            &input,
            cancelled,
        )
        .map_err(|error| match error {
            typed_layout::DispatchError::Layout(typed_layout::Error::Cancelled) => Error::Cancelled,
            other => Error::Dispatch(other),
        })?;
    if model
        .shapes
        .len()
        .saturating_add(model.descendants.len())
        .saturating_add(selected.output.shapes.len())
        > max_shapes
    {
        return Ok(());
    }
    let (descendants, skipped) =
        build_descendants(&parent, bounds, &input, selected.output, cancelled)?;
    let omitted = descendants
        .iter()
        .map(|shape| version.children(shape.item.item_no).len())
        .sum::<usize>()
        + skipped;
    model.descendants.extend(descendants);
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
    let mut metadata =
        Vec::with_capacity(model.shapes.len().saturating_add(model.descendants.len()));
    for meta in model
        .shapes
        .iter()
        .map(|s| &s.meta)
        .chain(model.descendants.iter().map(|s| &s.meta))
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

fn build_descendants(
    parent: &ParentShape,
    bounds: Rect,
    input: &Input,
    output: typed_layout::Output,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(Vec<DescendantShape>, usize), Error> {
    let mut visited = vec![false; input.rows().len()];
    let mut descendants = Vec::with_capacity(output.shapes.len());
    for child in output.shapes {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let source = input
            .rows()
            .get(child.row_index)
            .ok_or(Error::InvalidLayout)?;
        if source.item().result_version != parent.item.result_version
            || source.bytes() == 0
            || std::mem::replace(&mut visited[child.row_index], true)
        {
            return Err(Error::InvalidLayout);
        }
        let points = child.geometry.points();
        if child.geometry.primitive() != "rect"
            || points.len() != 2
            || points[0].x < bounds.left
            || points[0].y < bounds.top
            || points[1].x > bounds.right
            || points[1].y > bounds.bottom
        {
            return Err(Error::InvalidLayout);
        }
        descendants.push(DescendantShape {
            item: source.item().clone(),
            size: source.value().clone(),
            meta: ShapeMeta {
                shape_ref: format!(
                    "s{:x}-{}",
                    source.item().item_no,
                    parent.meta.visible_depth + 1
                ),
                parent_shape_ref: Some(parent.meta.shape_ref.clone()),
                visible_depth: parent.meta.visible_depth + 1,
                root_branch_ref: parent.meta.root_branch_ref.clone(),
            },
            geometry: child.geometry,
        });
    }
    for skip in &output.skipped {
        let source = input
            .rows()
            .get(skip.row_index)
            .ok_or(Error::InvalidLayout)?;
        if std::mem::replace(&mut visited[skip.row_index], true)
            || !matches!(
                (skip.reason, source.bytes()),
                (NoArea::ExactZero | NoArea::LowerBoundZero, 0)
                    | (NoArea::CoordinatePrecision, 1..)
            )
        {
            return Err(Error::InvalidLayout);
        }
    }
    if visited.iter().any(|v| !v) {
        return Err(Error::InvalidLayout);
    }
    Ok((descendants, output.skipped.len()))
}
