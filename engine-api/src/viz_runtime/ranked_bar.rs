//! Deferred single-parent ranked-bar publication for attributable exact sizes.
use super::*;
use crate::viz_pages::render;
use du_analysis_index::visualization::{
    bar_projection::{BarIdentity, ChildrenRequest},
    geometry::{Rect, Scale},
    projection, ratio, typed_layout,
};
use du_core_contracts::{
    SizeAmount, SizeBasis, SizeValue,
    job::JobOutcome,
    result::{ResultCompleteness, ResultItemKind},
    visualization::model::ShapeMeta,
};

pub(super) fn run(work: &compute::Compute, cancelled: &mut dyn FnMut() -> bool) -> JobOutcome {
    match prepare(work, cancelled) {
        Ok((candidate, model, count)) => match (work.candidate.lock(), work.work.computed.lock()) {
            (Ok(mut slot), Ok(mut computed)) => {
                *slot = Some(Candidate::Projection(candidate));
                computed.wire_model = model.map(Arc::new);
                let mut outcome = compute::outcome(true, None);
                outcome.counts.completed = count;
                outcome
            }
            _ => compute::outcome(false, Some("support.internal_error")),
        },
        Err(_) => compute::outcome(false, (!cancelled()).then_some("support.internal_error")),
    }
}

fn prepare(
    work: &compute::Compute,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<
    (
        crate::viz_pages::render::DeferredRender,
        Option<VizModel>,
        u64,
    ),
    EngineError,
> {
    let admitted = &work.admitted;
    let request = &admitted.request;
    let source = admitted.source.version();
    let mut evidence = HashMap::new();
    let mut presentations = Vec::new();
    let folder = du_analysis_index::browse::open(source, Some(&request.aggregate_scope_ref))
        .map_err(|_| internal())?;
    for item in folder.children {
        if cancelled() {
            return Err(internal());
        }
        let mut facts = work.producer.child(&admitted.source, item, request)?;
        if !admitted
            .facts
            .classes
            .contains_key(&facts.presentation.legend_class_key)
        {
            return Err(internal());
        }
        facts.presentation.display_flags.sort_unstable();
        facts.presentation.display_flags.dedup();
        presentations.push(render::ChildEvidence {
            item: du_core_contracts::ItemRef {
                result_version: admitted.source.result_version().clone(),
                item_no: item.item_no,
            },
            path_compare_key: facts.presentation.path_compare_key.clone(),
            legend_class_key: facts.presentation.legend_class_key.clone(),
            display_flags: facts.presentation.display_flags.clone(),
        });
        evidence.insert(item.item_no, facts);
    }
    let result = work.session.compute_ranked_bar(
        source,
        ChildrenRequest {
            version: source,
            scope: &request.aggregate_scope_ref,
            basis: request.size_basis.clone(),
            bounds: Rect {
                left: request.visible_window.left,
                top: request.visible_window.top,
                right: request.visible_window.right,
                bottom: request.visible_window.bottom,
            },
            scale: Scale::Automatic,
            top: admitted.max_shapes,
            max_bars: admitted.max_shapes,
        },
        &admitted.descriptor,
        cancelled,
    );
    let layout = match result {
        Ok(selected) => to_projection(admitted, selected, cancelled)?,
        Err(typed_layout::BarDispatchError::Bar(typed_layout::BarError::Projection(
            du_analysis_index::visualization::bar_projection::Error::GroupAttributionUnavailable,
        ))) => None,
        Err(_) => return Err(internal()),
    };
    let candidate = work
        .session
        .prepare_ranked(
            &admitted.source,
            render::RankedInput {
                parent: &request.aggregate_scope_ref,
                basis: &request.size_basis,
                fingerprint: admitted.key.clone(),
                layout,
                evidence: presentations,
            },
            cancelled,
        )
        .map_err(|_| internal())?;
    let reserved = work.session.reserve_prepared(candidate);
    let model = reserved
        .preview()
        .map(|preview| envelope::build_model(admitted, &preview, &evidence, cancelled))
        .transpose()?;
    Ok((
        work.session.defer_reserved(reserved),
        model,
        evidence.len() as u64,
    ))
}

fn exact(size: &SizeValue) -> Option<u64> {
    if size.evidence == du_core_contracts::SizeEvidence::Unknown {
        return None;
    }
    match &size.amount {
        SizeAmount::Exact { bytes } => bytes.as_str().parse().ok(),
        _ => None,
    }
}

fn to_projection(
    admitted: &Admitted,
    selected: typed_layout::SelectedBar,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<projection::Model>, EngineError> {
    let ranking = selected.output;
    let source = admitted.source.version();
    let parent = &admitted.request.aggregate_scope_ref;
    if selected.descriptor != admitted.descriptor
        || ranking.result_version != *admitted.source.result_version()
        || ranking.denominator != ranking.scope_total
    {
        return Err(internal());
    }
    if ranking
        .bars
        .iter()
        .any(|bar| matches!(bar.identity, BarIdentity::Other { .. }))
    {
        return Err(internal());
    }
    let Some(scope) = exact(&ranking.scope_total.size) else {
        return Ok(None);
    };
    if ranking.completeness != ResultCompleteness::Complete
        || ranking.ranking_incomplete
        || !ranking.unknown_items.is_empty()
    {
        return Ok(None);
    }
    let mut sum = 0_u64;
    let mut by_item = HashMap::new();
    for row in ranking.rows {
        if cancelled() {
            return Err(internal());
        }
        let Some(bytes) = exact(&row.size) else {
            return Ok(None);
        };
        sum = sum.checked_add(bytes).ok_or_else(internal)?;
        if by_item.insert(row.item.item_no, row).is_some() {
            return Err(internal());
        }
    }
    if sum != scope {
        return Ok(None);
    }
    let denominator =
        du_core_contracts::visualization::quantity::VizQuantity::exact("bytes", i128::from(scope))
            .ok_or_else(internal)?;
    let BarParts {
        rows,
        shapes,
        omissions,
        drawn,
    } = map_bars(source, ranking.bars, by_item, denominator, cancelled)?;
    let totals = source
        .selection_total(&drawn, true, cancelled)
        .map_err(|_| internal())?;
    let drawn = match &admitted.request.size_basis {
        SizeBasis::Logical => &totals.logical,
        SizeBasis::Allocated => &totals.allocated,
        SizeBasis::UniquePhysical => &totals.unique_physical,
    };
    let Some(drawn_bytes) = exact(drawn) else {
        return Ok(None);
    };
    Ok(Some(projection::Model {
        result_revision: ranking.result_version,
        parent: parent.clone(),
        basis: admitted.request.size_basis.clone(),
        completeness: ranking.completeness,
        observed_at: source.observed_at().cloned(),
        strategy: selected.descriptor,
        rows,
        shapes,
        descendants: vec![],
        relationships: vec![],
        omissions,
        totals: projection::Totals {
            scope: ranking.scope_total.size,
            drawn_bytes,
            grouped_bytes: 0,
            filtered_bytes: 0,
        },
    }))
}

struct BarParts {
    rows: Vec<projection::Row>,
    shapes: Vec<projection::Shape>,
    omissions: Vec<typed_layout::Omission>,
    drawn: Vec<du_core_contracts::ItemRef>,
}

fn map_bars(
    source: &du_analysis_index::ResultVersion,
    bars: Vec<du_analysis_index::visualization::bar_projection::Bar>,
    mut by_item: HashMap<u32, du_analysis_index::visualization::bar_projection::Row>,
    denominator: du_core_contracts::visualization::quantity::VizQuantity,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<BarParts, EngineError> {
    let mut rows = Vec::new();
    let mut shapes = Vec::new();
    let mut omissions = Vec::new();
    let mut drawn = Vec::new();
    let mut previous = None;
    for bar in bars {
        if cancelled() {
            return Err(internal());
        }
        let BarIdentity::Item(item) = bar.identity else {
            return Err(internal());
        };
        let row = by_item.remove(&item.item_no).ok_or_else(internal)?;
        if row.item != item {
            return Err(internal());
        }
        let value = exact(&row.size).ok_or_else(internal)?;
        if bar.value != du_analysis_index::visualization::geometry::Weight::Exact(value)
            || previous.is_some_and(|(prior_value, prior_item)| {
                value > prior_value || (value == prior_value && item.item_no < prior_item)
            })
        {
            return Err(internal());
        }
        previous = Some((value, item.item_no));
        if (bar.geometry.is_some() && (bar.no_area.is_some() || value == 0))
            || (bar.geometry.is_none() && bar.no_area.is_none())
        {
            return Err(internal());
        }
        let result_item = source.detail(&item).map_err(|_| internal())?.item;
        let branch = (result_item.kind == ResultItemKind::Directory).then(|| item.clone());
        let index = rows.len();
        rows.push(projection::Row {
            item: item.clone(),
            size: row.size,
            root_branch_ref: branch.clone(),
            area_value: if value == 0 {
                du_analysis_index::visualization::AreaValue::ExactZero
            } else {
                du_analysis_index::visualization::AreaValue::Positive
            },
            omission: bar.no_area,
            omitted_children: if result_item.kind == ResultItemKind::Directory {
                du_analysis_index::browse::open(source, Some(&item))
                    .map_err(|_| internal())?
                    .children
                    .len()
            } else {
                0
            },
            ratio: ratio::calculate(
                &du_core_contracts::visualization::quantity::VizQuantity::exact(
                    "bytes",
                    i128::from(value),
                )
                .ok_or_else(internal)?,
                &denominator,
            ),
        });
        if let Some(geometry) = bar.geometry {
            drawn.push(item.clone());
            shapes.push(projection::Shape {
                row_index: index,
                meta: ShapeMeta {
                    shape_ref: format!("s{:x}-1", item.item_no),
                    parent_shape_ref: None,
                    visible_depth: 1,
                    root_branch_ref: branch,
                },
                geometry,
            });
        } else if let Some(reason) = bar.no_area {
            omissions.push(typed_layout::Omission {
                row_index: index,
                reason,
            });
        } else {
            return Err(internal());
        }
    }
    if !by_item.is_empty() {
        return Err(internal());
    }
    Ok(BarParts {
        rows,
        shapes,
        omissions,
        drawn,
    })
}
