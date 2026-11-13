//! F24 preparation and response checks for a verified ranked Other group.
use super::*;
use crate::viz_pages::{production::RowPresentation, ranked_group, render};
use du_analysis_index::visualization::{
    bar_projection::{Bar, BarIdentity},
    geometry::{NoArea, Weight},
    typed_layout::SelectedBar,
};
use du_core_contracts::visualization::{
    model::{VizMatchKey, VizShape},
    render::VizFallback,
    response::VizRenderModelResponse,
    rows::VizRowsRequest,
};

pub(super) fn prepare(
    work: &compute::Compute,
    pinned: ranked_group::PinnedRankedBar,
    presentations: Vec<render::ChildEvidence>,
    evidence: &HashMap<u32, ItemEvidence>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<RankedWork, EngineError> {
    let admitted = &work.admitted;
    let request = &admitted.request;
    let selected = pinned.selected();
    if selected.output.ranking_incomplete
        || !selected.output.unknown_items.is_empty()
        || super::ranked_projection::exact(&selected.output.scope_total.size).is_none()
    {
        return fallback(work, presentations, evidence.len() as u64, cancelled);
    }
    let other = admitted.ranked_other.clone().ok_or_else(unavailable)?;
    let (expanded, group_index, group_size, group_geometry, group_count) =
        expand(selected, cancelled)?;
    let layout = super::ranked_projection::to_projection(admitted, expanded, cancelled)?;
    let Some(mut layout) = layout else {
        return fallback(work, presentations, evidence.len() as u64, cancelled);
    };
    let group_bytes = super::ranked_projection::exact(&group_size).ok_or_else(internal)?;
    layout.totals.grouped_bytes = group_bytes;
    if group_geometry.is_some() {
        layout.totals.drawn_bytes = layout
            .totals
            .drawn_bytes
            .checked_add(group_bytes)
            .ok_or_else(internal)?;
    }
    // Reuse the existing envelope from the same source rows, then append the
    // model-local group shape; F03 attribution is rechecked before reservation.
    let preview = render::Rendered {
        model_ref: "pending".into(),
        model: Arc::new(layout),
    };
    let mut model = envelope::build_model(admitted, &preview, evidence, cancelled)?;
    model.summary.grouped_items = du_core_contracts::visualization::quantity::VizQuantity::exact(
        "items",
        i128::from(group_count),
    )
    .ok_or_else(internal)?;
    if let Some(geometry) = group_geometry {
        model.shapes.push(VizShape {
            item_ref: None,
            group_ref: Some(other.group_ref.clone()),
            match_key: VizMatchKey::NotApplicable {},
            geometry,
            size: group_size,
            quantity: None,
            legend_class_key: other.legend_class_key.clone(),
            display_flags: other.display_flags.clone(),
            shape_ref: None,
            parent_shape_ref: None,
            visible_depth: None,
            root_branch_ref: None,
        });
    }
    model
        .validate_revision(admitted.source.result_version())
        .map_err(|_| internal())?;
    let row_evidence = presentations
        .into_iter()
        .map(|item| RowPresentation {
            item: item.item,
            legend_class_key: item.legend_class_key,
            display_flags: item.display_flags,
        })
        .collect();
    let group_ref = other.group_ref.clone();
    let candidate = work
        .session
        .defer_ranked_group(
            ranked_group::Request {
                pinned,
                scope: &request.aggregate_scope_ref,
                basis: request.size_basis.clone(),
                presentations: row_evidence,
                other,
                fingerprint: admitted.key.clone(),
                max_shapes: admitted.max_shapes,
            },
            cancelled,
        )
        .map_err(|_| internal())?;
    Ok((
        Candidate::RankedGroup(candidate),
        Some(model),
        evidence.len() as u64,
        Some((group_ref, group_index)),
    ))
}

fn fallback(
    work: &compute::Compute,
    evidence: Vec<render::ChildEvidence>,
    count: u64,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<RankedWork, EngineError> {
    let admitted = &work.admitted;
    let request = &admitted.request;
    let candidate = work
        .session
        .prepare_ranked(
            &admitted.source,
            render::RankedInput {
                parent: &request.aggregate_scope_ref,
                basis: &request.size_basis,
                fingerprint: admitted.key.clone(),
                layout: None,
                evidence,
            },
            cancelled,
        )
        .map_err(|_| internal())?;
    Ok((
        Candidate::Projection(
            work.session
                .defer_reserved(work.session.reserve_prepared(candidate)),
        ),
        None,
        count,
        None,
    ))
}

fn expand(
    selected: &SelectedBar,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<
    (
        SelectedBar,
        u32,
        du_core_contracts::SizeValue,
        Option<du_core_contracts::visualization::geometry::Geometry>,
        u64,
    ),
    EngineError,
> {
    let mut output = selected.output.clone();
    let mut expanded = Vec::new();
    let mut group = None;
    let mut rows = HashMap::new();
    for row in &selected.output.rows {
        if cancelled() {
            return Err(internal());
        }
        if rows.insert(row.item.item_no, row).is_some() {
            return Err(internal());
        }
    }
    for (index, bar) in selected.output.bars.iter().enumerate() {
        if cancelled() {
            return Err(internal());
        }
        match &bar.identity {
            BarIdentity::Item(_) => expanded.push(bar.clone()),
            BarIdentity::Other { members, size } => {
                if group.is_some() || members.is_empty() {
                    return Err(internal());
                }
                group = Some((
                    u32::try_from(index).map_err(|_| internal())?,
                    size.clone(),
                    bar.geometry.clone(),
                    u64::try_from(members.len()).map_err(|_| internal())?,
                ));
                for item in members {
                    if cancelled() {
                        return Err(internal());
                    }
                    let row = rows
                        .get(&item.item_no)
                        .filter(|row| row.item == *item)
                        .ok_or_else(internal)?;
                    let bytes = super::ranked_projection::exact(&row.size).ok_or_else(internal)?;
                    expanded.push(Bar {
                        identity: BarIdentity::Item(item.clone()),
                        value: Weight::Exact(bytes),
                        geometry: None,
                        no_area: Some(if bytes == 0 {
                            NoArea::ExactZero
                        } else {
                            NoArea::CoordinatePrecision
                        }),
                    });
                }
            }
        }
    }
    let (index, size, geometry, count) = group.ok_or_else(internal)?;
    output.bars = expanded;
    Ok((
        SelectedBar {
            descriptor: selected.descriptor.clone(),
            output,
        },
        index,
        size,
        geometry,
        count,
    ))
}

pub(super) fn response(
    session: &Session,
    admitted: &Admitted,
    fallback: &VizFallback,
    group_ref: &str,
    row_index: u32,
    model: Option<&VizModel>,
) -> Result<VizRenderResponse, EngineError> {
    let frame = session
        .ranked_frame(&fallback.model_ref)
        .map_err(|_| internal())?;
    if frame.descriptor != admitted.descriptor {
        return Err(internal());
    }
    let shape = frame
        .shapes
        .get(row_index as usize)
        .filter(|shape| shape.row_index == row_index as usize)
        .ok_or_else(internal)?;
    let page = session
        .rows(&VizRowsRequest {
            model_ref: fallback.model_ref.clone(),
            result_revision: fallback.result_revision.clone(),
            start: row_index,
            count: 1,
        })
        .map_err(|_| internal())?;
    let group_row = page
        .rows
        .first()
        .filter(|row| row.group_ref.as_deref() == Some(group_ref))
        .ok_or_else(internal)?;
    let members = session
        .group_rows(&fallback.model_ref, group_ref, 0, 1)
        .map_err(|_| internal())?;
    if members.result_revision != fallback.result_revision || members.group_total != group_row.size
    {
        return Err(internal());
    }
    let mut model = model.ok_or_else(internal)?.clone();
    let group_shape = model
        .shapes
        .iter()
        .find(|item| item.group_ref.as_deref() == Some(group_ref));
    if group_shape.map(|item| &item.geometry) != shape.geometry.as_ref() {
        return Err(internal());
    }
    model.model_ref.clone_from(&fallback.model_ref);
    let response = VizRenderResponse::Model(VizRenderModelResponse::Model {
        request_seq: admitted.request.request_seq,
        result_revision: fallback.result_revision.clone(),
        observed_at: admitted.source.version().observed_at().cloned(),
        completeness: admitted.source.version().completeness(),
        model: Box::new(model),
    });
    response.validate().map_err(|_| internal())?;
    Ok(response)
}
