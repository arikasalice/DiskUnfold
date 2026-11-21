//! Sankey work admitted by the live visualization runtime (#296): the scope's space by
//! folder and by type, counted as the type legend counts it.
use super::*;
use du_analysis_index::visualization::{
    folder_types,
    geometry::{Rect, Weight},
    sankey, type_legend,
    typed_layout::{SankeyDispatchError, SankeyInput},
};
use du_core_contracts::{
    ItemRef,
    job::JobOutcome,
    result::{ResultItemKind, ResultVersionRead},
};

/// A node's width in CSS px (F06 README 12, row "생키").
const NODE_WIDTH_PX: u64 = 12;
/// Nodes besides the folders: the scope, the types and the two unattributed nodes.
const FIXED_NODES: usize = 1 + type_legend::LISTED.len() + 2;

pub(super) fn run(work: &compute::Compute, cancelled: &mut dyn FnMut() -> bool) -> JobOutcome {
    let failed =
        |cancelled: bool| compute::outcome(false, (!cancelled).then_some("support.internal_error"));
    let (input, model) = match prepare(work, cancelled) {
        Ok(Some(prepared)) => prepared,
        // A graph that cannot be formed shows the folder's table, as missing data does.
        Ok(None) => return compute::missing_data_fallback(work, cancelled),
        Err(_) => return failed(cancelled()),
    };
    let scope = work.admitted.request.aggregate_scope_ref.clone();
    let deferred = match work.session.defer_sankey(input, &scope, cancelled) {
        Ok(deferred) => deferred,
        Err(_) => return failed(cancelled()),
    };
    match (work.candidate.lock(), work.work.computed.lock()) {
        (Ok(mut slot), Ok(mut computed)) => {
            *slot = Some(Candidate::Sankey(deferred));
            computed.wire_model = Some(Arc::new(model));
            computed.histogram_revision = Some(work.admitted.source.result_version().clone());
            compute::outcome(true, None)
        }
        _ => compute::outcome(false, Some("support.internal_error")),
    }
}

/// The node rows and the model, or `None` when no conserved graph can be formed: a
/// sample, a scope or a file size that is not exact, a folder with unread ranges, or
/// a viewport too small for the nodes.
fn prepare(
    work: &compute::Compute,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<(crate::viz_pages::ModelInput, VizModel)>, EngineError> {
    let admitted = &work.admitted;
    let request = &admitted.request;
    let version = admitted.source.version();
    let basis = &request.size_basis;
    if !version.data_source().is_real() {
        return Ok(None);
    }
    let Some(scope_bytes) = super::sankey_model::exact(
        version.item_size(request.aggregate_scope_ref.item_no, basis.clone()),
    ) else {
        return Ok(None);
    };
    let Some(rows) = attributed_rows(admitted, cancelled)? else {
        return Ok(None);
    };
    let Some(folder_budget) = admitted
        .max_shapes
        .checked_sub(FIXED_NODES)
        .filter(|n| *n >= 1)
    else {
        return Ok(None);
    };
    let order: Vec<u32> = (0..type_legend::LISTED.len() as u32).collect();
    let selected = match work.session.compute_sankey_selected(
        SankeyInput {
            columns: columns(request),
            scope: Weight::Exact(scope_bytes),
            rows: &rows,
            folder_budget,
            category_order: &order,
        },
        &admitted.descriptor,
        cancelled,
    ) {
        Ok(selected) => selected,
        Err(SankeyDispatchError::Cancelled) => return Err(internal()),
        Err(_) => return Ok(None),
    };
    super::sankey_model::build(admitted, &selected.output, scope_bytes, cancelled).map(Some)
}

/// One row per (child, type): a file is its own type, a folder adds the files below it.
fn attributed_rows(
    admitted: &Admitted,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<Vec<sankey::Row>>, EngineError> {
    let request = &admitted.request;
    let version = admitted.source.version();
    let basis = &request.size_basis;
    let folder = du_analysis_index::browse::open(version, Some(&request.aggregate_scope_ref))
        .map_err(|_| internal())?;
    let mut rows = Vec::new();
    for (order, child) in folder.children.iter().enumerate() {
        if cancelled() {
            return Err(internal());
        }
        let folder_order = u32::try_from(order).map_err(|_| internal())?;
        let row = |key: &str, bytes: u64| sankey::Row {
            folder: child.item_no,
            folder_order,
            category: category(key),
            bytes: Some(i128::from(bytes)),
        };
        match child.kind {
            ResultItemKind::Directory => {
                let reference = ItemRef {
                    result_version: admitted.source.result_version().clone(),
                    item_no: child.item_no,
                };
                let types = folder_types::folder_types(version, &reference, basis, cancelled)
                    .map_err(|_| internal())?;
                let Some(types) = types.filter(|types| !types.below_unread) else {
                    return Ok(None);
                };
                for (key, total) in types.classes {
                    if total.lower_bound {
                        return Ok(None);
                    }
                    rows.push(row(key, total.bytes));
                }
            }
            ResultItemKind::File => {
                let Some(bytes) =
                    super::sankey_model::exact(version.item_size(child.item_no, basis.clone()))
                else {
                    return Ok(None);
                };
                let key = type_legend::file_type_of(version, child.item_no)
                    .map_or(type_legend::NO_CLASS, type_legend::key_of);
                rows.push(row(key, bytes));
            }
            // A link that was not followed, or an item of unknown kind, has no space here.
            _ => {}
        }
    }
    Ok(Some(rows))
}

/// The place of a type in the legend, which is the order of the type column.
pub(super) fn category(key: &str) -> u32 {
    let last = type_legend::LISTED.len() - 1;
    type_legend::LISTED
        .iter()
        .position(|class| class.key == key)
        .unwrap_or(last) as u32
}

/// Three node columns across the visible window, each one node wide.
fn columns(request: &VizRenderRequest) -> sankey::Columns {
    let window = &request.visible_window;
    let span = u64::from(window.right.saturating_sub(window.left));
    let width = u32::try_from(span * NODE_WIDTH_PX / u64::from(request.viewport.width.get()))
        .unwrap_or(u32::MAX)
        .max(1);
    let column = |left: u32| Rect {
        left,
        top: window.top,
        right: left.saturating_add(width).min(window.right),
        bottom: window.bottom,
    };
    let middle = window.left + (window.right - window.left) / 2 - width / 2;
    sankey::Columns {
        volume: column(window.left),
        folders: column(middle),
        categories: column(window.right.saturating_sub(width)),
    }
}
