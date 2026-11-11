//! Produces the existing model envelope from the same fixed histogram rows.
use super::*;
use crate::viz_pages::histogram::SelectedCandidate;
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeValue,
    browse_page_ipc::{QueryStrategy, ResultOrigin},
    result::{ResultCompleteness, SpaceAggregate},
    visualization::{
        geometry::{Geometry, Point},
        model::*,
        quantity::VizQuantity,
    },
};
use std::collections::BTreeMap;

pub(super) fn build(
    admitted: &Admitted,
    candidate: &SelectedCandidate,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<VizModel, EngineError> {
    let request = &admitted.request;
    let policy = admitted.histogram.as_ref().ok_or_else(internal)?;
    let data = candidate.model();
    let pages = candidate.pages();
    let complete = data.histogram.source_completeness == ResultCompleteness::Complete
        && data.histogram.uncertain_items.is_empty()
        && data.histogram.unclassified_items.is_empty();
    let mut drawn = Vec::new();
    let mut grouped = Vec::new();
    let mut classes: BTreeMap<String, Vec<ItemRef>> = BTreeMap::new();
    let mut shapes = Vec::new();
    for (bin, row) in data.histogram.bins.iter().zip(&pages.rows) {
        if cancelled() {
            return Err(internal());
        }
        let key = row.group_ref.as_ref().ok_or_else(internal)?;
        let group = pages.groups.get(key).ok_or_else(internal)?;
        let members: Vec<_> = group
            .rows
            .iter()
            .map(|row| row.item_ref.clone().ok_or_else(internal))
            .collect::<Result<_, _>>()?;
        grouped.extend(members.iter().cloned());
        classes
            .entry(row.legend_class_key.clone())
            .or_default()
            .extend(members.iter().cloned());
        if let Some(rect) = bin.rect {
            drawn.extend(members);
            let geometry = Geometry::new(
                "rect",
                vec![
                    Point {
                        x: rect.left,
                        y: rect.top,
                    },
                    Point {
                        x: rect.right,
                        y: rect.bottom,
                    },
                ],
            )
            .map_err(|_| internal())?;
            shapes.push(VizShape {
                item_ref: None,
                group_ref: Some(key.clone()),
                match_key: VizMatchKey::NotApplicable {},
                geometry,
                size: row.size.clone(),
                quantity: row.quantity.clone(),
                legend_class_key: row.legend_class_key.clone(),
                display_flags: row.display_flags.clone(),
                shape_ref: None,
                parent_shape_ref: None,
                visible_depth: None,
                root_branch_ref: None,
            });
        }
    }
    let aggregate = |members: &[ItemRef],
                     complete: bool,
                     cancelled: &mut dyn FnMut() -> bool|
     -> Result<SizeValue, EngineError> {
        let result = admitted
            .source
            .version()
            .selection_total(members, complete, cancelled)
            .map_err(|_| internal())?;
        Ok(select(result, &request.size_basis))
    };
    let total = |size: SizeValue| total(size, &policy.unknown_reason);
    let scope_size = select(data.summary.aggregate.clone(), &request.size_basis);
    let scope = total(scope_size)?;
    let drawn_total = total(aggregate(&drawn, complete, cancelled)?)?;
    let grouped_total = total(aggregate(&grouped, complete, cancelled)?)?;
    let filtered_total = total(aggregate(&[], true, cancelled)?)?;
    let mut legend = Vec::new();
    for (key, members) in classes {
        let class = admitted.facts.classes.get(&key).ok_or_else(internal)?;
        legend.push(VizLegendEntry {
            key,
            label_key: class.label_key.clone(),
            item_count: count(members.len() as u64, complete)?,
            total: total(aggregate(&members, complete, cancelled)?)?,
            condition: class.condition.clone(),
        });
    }
    let denominator = pages.rows.first().ok_or_else(internal)?.denominator.clone();
    let model = VizModel {
        model_ref: "pending".into(),
        condition_fingerprint: admitted.key.clone(),
        aggregate_scope_ref: request.aggregate_scope_ref.clone(),
        navigation_scope_ref: request.navigation_scope_ref.clone(),
        focus_item_ref: request.focus_item_ref.clone(),
        origin: ResultOrigin("scan".into()),
        staleness: admitted.facts.staleness.clone(),
        representation: VizRepresentation {
            format: request.format.clone(),
            measure: request.measure.clone(),
            size_basis: request.size_basis.clone(),
            classification_criterion: request.classification_criterion.clone(),
            color_criterion: request.color_criterion.clone(),
            denominator_basis: request.denominator_basis,
            color_mode: None,
            label_mode: None,
        },
        denominator,
        totals: VizTotals {
            scope,
            drawn: drawn_total,
            grouped: grouped_total,
            filtered_out: filtered_total,
        },
        shapes,
        relationships: vec![],
        legend,
        summary: VizSummary {
            grouped_items: count(grouped.len() as u64, complete)?,
            omitted_items: count(0, true)?,
            unknown_items: count(
                (data.histogram.unclassified_items.len() + data.histogram.uncertain_items.len())
                    as u64,
                complete,
            )?,
            reason_keys: vec![],
        },
        strategy: QueryStrategy {
            id: admitted.descriptor.strategy.id.clone(),
            revision: admitted.descriptor.strategy.revision,
        },
        table_revisions: admitted.facts.tables.clone(),
    };
    model
        .validate_revision(admitted.source.result_version())
        .map_err(|_| internal())?;
    Ok(model)
}

fn select(aggregate: SpaceAggregate, basis: &SizeBasis) -> SizeValue {
    match basis {
        SizeBasis::Logical => aggregate.logical,
        SizeBasis::Allocated => aggregate.allocated,
        SizeBasis::UniquePhysical => aggregate.unique_physical,
    }
}
fn total(size: SizeValue, reason: &str) -> Result<VizTotal, EngineError> {
    let quantity = match &size.amount {
        SizeAmount::Exact { bytes } => {
            VizQuantity::exact("bytes", bytes.as_str().parse().map_err(|_| internal())?)
        }
        SizeAmount::LowerBound { bytes } => {
            VizQuantity::lower_bound("bytes", bytes.as_str().parse().map_err(|_| internal())?)
        }
        SizeAmount::Unknown => VizQuantity::unknown("bytes", reason.into()),
    }
    .ok_or_else(internal)?;
    Ok(VizTotal {
        quantity,
        source_size: Some(size),
    })
}
fn count(value: u64, complete: bool) -> Result<VizQuantity, EngineError> {
    if complete {
        VizQuantity::exact("items", i128::from(value))
    } else {
        VizQuantity::lower_bound("items", value)
    }
    .ok_or_else(internal)
}
