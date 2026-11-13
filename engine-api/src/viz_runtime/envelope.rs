use super::*;
use du_core_contracts::{
    SizeAmount, SizeValue,
    browse_page_ipc::{QueryStrategy, ResultOrigin},
    visualization::{
        model::*,
        quantity::VizQuantity,
        render::VizRenderNonModelResponse,
        response::VizRenderModelResponse,
        rows::{VizDenominator, VizDenominatorBasis},
    },
};
pub(super) fn job(admitted: &Admitted, job_ref: Id) -> VizRenderResponse {
    VizRenderResponse::NonModel(VizRenderNonModelResponse::Job {
        request_seq: admitted.request.request_seq.get(),
        result_revision: admitted.source.result_version().clone(),
        observed_at: admitted.source.version().observed_at().cloned(),
        completeness: admitted.source.version().completeness(),
        job_ref,
    })
}
pub(super) fn response(
    session: &Session,
    admitted: &Admitted,
    committed: &CommittedRender,
    wire_model: Option<&VizModel>,
) -> Result<VizRenderResponse, EngineError> {
    let response = match committed {
        CommittedRender::Fallback { reason, fallback } => {
            if fallback.result_revision != *admitted.source.result_version() {
                return Err(internal());
            }
            // Resolving a single row revalidates model/source identity, including an
            // empty fallback. A stale or evicted model is never returned as a hit.
            session
                .rows(&du_core_contracts::visualization::rows::VizRowsRequest {
                    model_ref: fallback.model_ref.clone(),
                    result_revision: fallback.result_revision.clone(),
                    start: 0,
                    count: 1,
                })
                .map_err(page_error)?;
            VizRenderResponse::NonModel(VizRenderNonModelResponse::Unavailable {
                request_seq: admitted.request.request_seq.get(),
                result_revision: fallback.result_revision.clone(),
                observed_at: admitted.source.version().observed_at().cloned(),
                completeness: admitted.source.version().completeness(),
                reason_key: match admitted
                    .required_data
                    .as_ref()
                    .map(|value| value.evaluation)
                {
                    Some(crate::required_data::Evaluation::Unavailable(reason)) => reason.key(),
                    Some(crate::required_data::Evaluation::Available) | None => {
                        fallback_reason(*reason)
                    }
                }
                .into(),
                available_formats: admitted
                    .required_data
                    .as_ref()
                    .map(|value| value.formats.as_slice())
                    .unwrap_or(&admitted.facts.available_formats)
                    .iter()
                    .filter(|format| **format != admitted.request.format)
                    .cloned()
                    .collect(),
                fallback: Some(fallback.clone()),
            })
        }
        CommittedRender::Model(rendered) => {
            if matches!(
                admitted
                    .required_data
                    .as_ref()
                    .map(|value| value.evaluation),
                Some(crate::required_data::Evaluation::Unavailable(_))
            ) {
                return Err(internal());
            }
            let mut model = wire_model.ok_or_else(internal)?.clone();
            model.model_ref.clone_from(&rendered.model_ref);
            session
                .rows(&du_core_contracts::visualization::rows::VizRowsRequest {
                    model_ref: rendered.model_ref.clone(),
                    result_revision: admitted.source.result_version().clone(),
                    start: 0,
                    count: 1,
                })
                .map_err(page_error)?;
            VizRenderResponse::Model(VizRenderModelResponse::Model {
                request_seq: admitted.request.request_seq,
                result_revision: admitted.source.result_version().clone(),
                observed_at: admitted.source.version().observed_at().cloned(),
                completeness: admitted.source.version().completeness(),
                model: Box::new(model),
            })
        }
    };
    response.validate().map_err(|_| internal())?;
    Ok(response)
}
fn fallback_reason(reason: crate::viz_pages::render::FallbackReason) -> &'static str {
    use crate::viz_pages::render::FallbackReason::*;
    match reason {
        NoAttribution | OrderingUnavailable => "viz.unavailable.no_attribution",
    }
}
pub(super) fn build_model(
    admitted: &Admitted,
    rendered: &crate::viz_pages::render::Rendered,
    evidence: &HashMap<u32, ItemEvidence>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<VizModel, EngineError> {
    let projection = &rendered.model;
    let request = &admitted.request;
    let mut shapes = Vec::with_capacity(projection.shapes.len());
    let mut drawn_lower_bound = false;
    for shape in &projection.shapes {
        if cancelled() {
            return Err(internal());
        }
        let row = projection.rows.get(shape.row_index).ok_or_else(internal)?;
        drawn_lower_bound |= matches!(row.size.amount, SizeAmount::LowerBound { .. });
        let item = evidence.get(&row.item.item_no).ok_or_else(internal)?;
        let hierarchy = request.format == "treemap";
        shapes.push(VizShape {
            item_ref: Some(row.item.clone()),
            group_ref: None,
            match_key: item.match_key.clone(),
            geometry: shape.geometry.clone(),
            size: row.size.clone(),
            quantity: None,
            legend_class_key: item.presentation.legend_class_key.clone(),
            display_flags: item.presentation.display_flags.clone(),
            shape_ref: hierarchy.then(|| shape.meta.shape_ref.clone()),
            parent_shape_ref: hierarchy.then(|| shape.meta.parent_shape_ref.clone()),
            visible_depth: hierarchy.then_some(shape.meta.visible_depth),
            root_branch_ref: hierarchy.then(|| shape.meta.root_branch_ref.clone()),
        });
    }
    for shape in &projection.descendants {
        if cancelled() {
            return Err(internal());
        }
        let item = evidence.get(&shape.item.item_no).ok_or_else(internal)?;
        shapes.push(VizShape {
            item_ref: Some(shape.item.clone()),
            group_ref: None,
            match_key: item.match_key.clone(),
            geometry: shape.geometry.clone(),
            size: shape.size.clone(),
            quantity: None,
            legend_class_key: item.presentation.legend_class_key.clone(),
            display_flags: item.presentation.display_flags.clone(),
            shape_ref: Some(shape.meta.shape_ref.clone()),
            parent_shape_ref: Some(shape.meta.parent_shape_ref.clone()),
            visible_depth: Some(shape.meta.visible_depth),
            root_branch_ref: Some(shape.meta.root_branch_ref.clone()),
        });
    }
    let mut classes = std::collections::BTreeMap::<String, (u64, u64, bool)>::new();
    for row in &projection.rows {
        if cancelled() {
            return Err(internal());
        }
        let item = evidence.get(&row.item.item_no).ok_or_else(internal)?;
        let total = classes
            .entry(item.presentation.legend_class_key.clone())
            .or_default();
        total.0 = total.0.checked_add(1).ok_or_else(internal)?;
        total.1 = total
            .1
            .checked_add(bytes(&row.size)?)
            .ok_or_else(internal)?;
        total.2 |= matches!(row.size.amount, SizeAmount::LowerBound { .. });
    }
    let mut legend = Vec::with_capacity(classes.len());
    for (key, (count, total, lower)) in classes {
        if cancelled() {
            return Err(internal());
        }
        let class = admitted.facts.classes.get(&key).ok_or_else(internal)?;
        legend.push(VizLegendEntry {
            key,
            label_key: class.label_key.clone(),
            item_count: quantity("items", count)?,
            total: derived_total(&projection.totals.scope, total, lower)?,
            condition: class.condition.clone(),
        });
    }
    let scope = original_total(&projection.totals.scope)?;
    let model = VizModel {
        model_ref: rendered.model_ref.clone(),
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
            color_mode: request.color_mode,
            label_mode: request.label_mode,
        },
        denominator: VizDenominator {
            basis: VizDenominatorBasis::Scope,
            value: scope.quantity.clone(),
            size: scope.source_size.clone(),
        },
        totals: VizTotals {
            scope,
            drawn: derived_total(
                &projection.totals.scope,
                projection.totals.drawn_bytes,
                drawn_lower_bound,
            )?,
            grouped: derived_total(
                &projection.totals.scope,
                projection.totals.grouped_bytes,
                false,
            )?,
            filtered_out: derived_total(
                &projection.totals.scope,
                projection.totals.filtered_bytes,
                false,
            )?,
        },
        shapes,
        relationships: projection.relationships.clone(),
        legend,
        summary: VizSummary {
            grouped_items: quantity("items", 0)?,
            omitted_items: quantity(
                "items",
                projection
                    .rows
                    .iter()
                    .map(|row| row.omitted_children as u64)
                    .sum(),
            )?,
            unknown_items: quantity("items", 0)?,
            reason_keys: vec![],
        },
        strategy: QueryStrategy {
            id: projection.strategy.strategy.id.clone(),
            revision: projection.strategy.strategy.revision,
        },
        table_revisions: admitted.facts.tables.clone(),
    };
    model
        .validate_revision(admitted.source.result_version())
        .map_err(|_| internal())?;
    Ok(model)
}
fn bytes(size: &SizeValue) -> Result<u64, EngineError> {
    match &size.amount {
        SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => {
            bytes.as_str().parse().map_err(|_| internal())
        }
        _ => Err(internal()),
    }
}
fn quantity(unit: &str, value: u64) -> Result<VizQuantity, EngineError> {
    VizQuantity::exact(unit, i128::from(value)).ok_or_else(internal)
}
fn original_total(size: &SizeValue) -> Result<VizTotal, EngineError> {
    Ok(VizTotal {
        quantity: if matches!(size.amount, SizeAmount::LowerBound { .. }) {
            VizQuantity::lower_bound("bytes", bytes(size)?).ok_or_else(internal)?
        } else {
            quantity("bytes", bytes(size)?)?
        },
        source_size: Some(size.clone()),
    })
}
fn derived_total(source: &SizeValue, value: u64, lower: bool) -> Result<VizTotal, EngineError> {
    original_total(&SizeValue {
        basis: source.basis.clone(),
        evidence: source.evidence.clone(),
        amount: if lower {
            SizeAmount::LowerBound {
                bytes: value.into(),
            }
        } else {
            SizeAmount::Exact {
                bytes: value.into(),
            }
        },
    })
}
pub(super) fn page_error(error: crate::viz_pages::Error) -> EngineError {
    EngineError::new(
        match error {
            crate::viz_pages::Error::StaleVersion => "result.version_stale",
            crate::viz_pages::Error::ModelExpired => "viz.model_expired",
            _ => "support.internal_error",
        },
        vec![],
    )
}
