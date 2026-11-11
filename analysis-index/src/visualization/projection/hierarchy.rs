use super::*;

/// Extends an admitted root shape with its immediate children from the same reader.
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
    let shape = model.shapes.get(root_shape).ok_or(Error::InvalidLayout)?;
    let row = model
        .rows
        .get(shape.row_index)
        .ok_or(Error::InvalidLayout)?;
    if version
        .items()
        .get(row.item.item_no as usize)
        .is_none_or(|item| item.kind != ResultItemKind::Directory)
    {
        return Err(Error::InvalidSource);
    }
    let points = shape.geometry.points();
    if shape.geometry.primitive() != "rect" || points.len() != 2 {
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
        &row.item,
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
        build_descendants(shape, row, bounds, &input, selected.output, cancelled)?;
    let row_index = shape.row_index;
    let omitted = descendants
        .iter()
        .map(|shape| version.children(shape.item.item_no).len())
        .sum::<usize>()
        + skipped;
    model.descendants.extend(descendants);
    model.rows[row_index].omitted_children = omitted;
    Ok(())
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
    shape: &Shape,
    row: &Row,
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
        if source.item().result_version != row.item.result_version
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
                shape_ref: format!("s{:x}-2", source.item().item_no),
                parent_shape_ref: Some(shape.meta.shape_ref.clone()),
                visible_depth: 2,
                root_branch_ref: Some(row.item.clone()),
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
