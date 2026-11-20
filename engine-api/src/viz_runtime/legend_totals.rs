//! Per-class counts and byte totals of a model's legend (F06 FR6, FR14).
use super::*;
use du_analysis_index::visualization::{geometry::NoArea, projection};
use du_core_contracts::SizeAmount;
use std::collections::BTreeMap;

/// Item count, summed bytes and whether the sum is only a lower bound, per class key.
pub(super) type Classes = BTreeMap<String, (u64, u64, bool)>;

/// The legend of a model. With the type criterion a row counts its type's files at
/// any depth below the scope, filtered or not, so it matches what narrowing to it
/// draws. Without it, the rows the model shows, or with a filter the scope's direct
/// children, so the legend keeps the values of the whole scope to compare against.
pub(super) fn classes(
    admitted: &Admitted,
    projection: &projection::Model,
    evidence: &HashMap<u32, ItemEvidence>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Classes, EngineError> {
    if admitted.request.color_criterion == crate::viz_production::policy::COLOR_CRITERION_TYPE {
        subtree_classes(admitted, cancelled)
    } else if admitted.filter.is_some() {
        scope_classes(admitted, evidence, cancelled)
    } else {
        row_classes(&projection.rows, evidence, cancelled)
    }
}

fn subtree_classes(
    admitted: &Admitted,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Classes, EngineError> {
    let totals = du_analysis_index::visualization::type_legend::scope_totals(
        admitted.source.version(),
        &admitted.request.aggregate_scope_ref,
        &admitted.request.size_basis,
        cancelled,
    )
    .map_err(|_| internal())?;
    Ok(totals
        .into_iter()
        .map(|(key, total)| {
            (
                key.to_owned(),
                (total.count, total.bytes, total.lower_bound),
            )
        })
        .collect())
}

fn row_classes(
    rows: &[projection::Row],
    evidence: &HashMap<u32, ItemEvidence>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Classes, EngineError> {
    let mut classes = Classes::new();
    for row in rows {
        if cancelled() {
            return Err(internal());
        }
        let item = evidence.get(&row.item.item_no).ok_or_else(internal)?;
        let total = classes
            .entry(item.presentation.legend_class_key.clone())
            .or_default();
        total.0 = total.0.checked_add(1).ok_or_else(internal)?;
        if row.omission == Some(NoArea::Unknown) {
            // Counted, never added: the bytes of a sibling without a provable area
            // are not known to be disjoint from the others, so the class total is
            // only a lower bound.
            total.2 = true;
            continue;
        }
        add(total, &row.size.amount)?;
    }
    Ok(classes)
}

/// The scope's direct children by their F03 values. The filtered rows cannot give
/// them: they hold kept bytes only, and leave out children that keep nothing.
fn scope_classes(
    admitted: &Admitted,
    evidence: &HashMap<u32, ItemEvidence>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Classes, EngineError> {
    let version = admitted.source.version();
    let basis = &admitted.request.size_basis;
    let folder =
        du_analysis_index::browse::open(version, Some(&admitted.request.aggregate_scope_ref))
            .map_err(|_| internal())?;
    let mut classes = Classes::new();
    for child in folder.children {
        if cancelled() {
            return Err(internal());
        }
        let item = evidence.get(&child.item_no).ok_or_else(internal)?;
        let total = classes
            .entry(item.presentation.legend_class_key.clone())
            .or_default();
        total.0 = total.0.checked_add(1).ok_or_else(internal)?;
        match version.item_size(child.item_no, basis.clone()) {
            Some(size) if size.evidence != du_core_contracts::SizeEvidence::Unknown => {
                add(total, &size.amount)?;
            }
            _ => total.2 = true,
        }
    }
    Ok(classes)
}

fn add(total: &mut (u64, u64, bool), amount: &SizeAmount) -> Result<(), EngineError> {
    match amount {
        SizeAmount::Exact { bytes } => {
            total.1 = total.1.checked_add(bytes.get()).ok_or_else(internal)?;
        }
        SizeAmount::LowerBound { bytes } => {
            total.1 = total.1.checked_add(bytes.get()).ok_or_else(internal)?;
            total.2 = true;
        }
        SizeAmount::Unknown => total.2 = true,
    }
    Ok(())
}
