use super::unread::{first_unknown_row, unknown_size};
use super::*;
use du_core_contracts::{
    SizeAmount, SizeValue,
    browse_page_ipc::{QueryStrategy, ResultOrigin},
    visualization::{
        item_kind::VizItemKind,
        model::*,
        quantity::{VizAmount, VizQuantity},
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
                    parent_shape_ref: None,
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
                    parent_shape_ref: None,
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
        NoAttribution => "viz.unavailable.no_attribution",
    }
}
/// Kind of a pinned result item for its shape; an item the version lacks has none.
fn kind_of(admitted: &Admitted, item_no: u32) -> Option<VizItemKind> {
    let items = admitted.source.version().items();
    let item = items.get(item_no as usize)?;
    VizItemKind::from_result(item.kind)
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
    for shape in &projection.shapes {
        if cancelled() {
            return Err(internal());
        }
        let row = projection.rows.get(shape.row_index).ok_or_else(internal)?;
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
            member_count: None,
            item_kind: kind_of(admitted, row.item.item_no),
            type_slices: None,
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
            member_count: None,
            item_kind: kind_of(admitted, shape.item.item_no),
            type_slices: None,
        });
    }
    for group in &projection.detail.groups {
        if cancelled() {
            return Err(internal());
        }
        // A group exists only when its presentation was supplied for this request.
        let other = admitted.ranked_other.as_ref().ok_or_else(internal)?;
        let mut display_flags = other.display_flags.clone();
        if matches!(group.total.amount, SizeAmount::LowerBound { .. }) {
            display_flags.push(crate::viz_production::policy::FLAG_LOWER_BOUND.to_owned());
        }
        display_flags.sort_unstable();
        display_flags.dedup();
        shapes.push(VizShape {
            item_ref: None,
            group_ref: Some(group.group_ref.clone()),
            match_key: VizMatchKey::NotApplicable {},
            geometry: group.geometry.clone(),
            size: group.total.clone(),
            quantity: None,
            legend_class_key: other.legend_class_key.clone(),
            display_flags,
            shape_ref: Some(group.meta.shape_ref.clone()),
            parent_shape_ref: Some(group.meta.parent_shape_ref.clone()),
            visible_depth: Some(group.meta.visible_depth),
            root_branch_ref: Some(group.meta.root_branch_ref.clone()),
            member_count: Some(quantity("items", group.members.len() as u64)?),
            item_kind: None,
            type_slices: None,
        });
    }
    super::shape_slices::add(admitted, &mut shapes, cancelled)?;
    let classes = super::legend_totals::classes(admitted, projection, evidence, cancelled)?;
    let mut legend = Vec::with_capacity(classes.len());
    for (key, (count, total, lower)) in
        super::legend_order::listed(classes, &admitted.facts.legend_order)
    {
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
    let drawn = derived_total(
        &projection.totals.scope,
        projection.totals.drawn_bytes,
        projection.totals.drawn_lower_bound,
    )?;
    let grouped = derived_total(
        &projection.totals.scope,
        projection.totals.grouped_bytes,
        projection.totals.grouped_lower_bound,
    )?;
    let filtered_out = derived_total(
        &projection.totals.scope,
        projection.totals.filtered_bytes,
        false,
    )?;
    let remainder = super::remainder::compute(
        &scope,
        [&drawn, &grouped, &filtered_out],
        projection.detail.remainder_causes,
        REMAINDER_UNKNOWN_REASON,
    )?;
    let grouped_items = grouped_items(&shapes)?;
    // The folders the scope could not read (F06 README 48), on treemap models only.
    let (unread_folders, first_unread_row) = if request.format == "treemap" {
        super::unread::unread_folders(admitted.source.version(), projection)?
    } else {
        (None, None)
    };
    let model = VizModel {
        model_ref: rendered.model_ref.clone(),
        condition_fingerprint: admitted.key.clone(),
        aggregate_scope_ref: request.aggregate_scope_ref.clone(),
        navigation_scope_ref: request.navigation_scope_ref.clone(),
        focus_item_ref: request.focus_item_ref.clone(),
        origin: ResultOrigin("scan".into()),
        data_source: Some(admitted.source.version().data_source()),
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
            drawn,
            grouped,
            filtered_out,
            remainder: Some(remainder.total),
            remainder_reason_keys: remainder.reason_keys,
            scope_measurement_limits: crate::space::item_limits(
                admitted.source.version(),
                request.aggregate_scope_ref.item_no,
                &request.size_basis,
            ),
        },
        shapes,
        relationships: projection.relationships.clone(),
        legend,
        summary: VizSummary {
            grouped_items,
            omitted_items: quantity(
                "items",
                projection
                    .rows
                    .iter()
                    .map(|row| row.omitted_children as u64)
                    .sum::<u64>()
                    .checked_add(projection.detail.omitted_items)
                    .ok_or_else(internal)?,
            )?,
            unknown_items: quantity("items", projection.detail.unknown_items)?,
            reason_keys: projection.detail.reason_keys.clone(),
            unknown_size: (request.format == "treemap")
                .then(|| unknown_size(projection.detail.unknown_share))
                .flatten()
                .transpose()?,
            first_unknown_row: if request.format == "treemap" {
                first_unknown_row(projection)?
            } else {
                None
            },
            unread_folders,
            first_unread_row,
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
/// The reason of an unknown remainder: F03's reason for a size that cannot be stated.
/// A scope or a total that is a lower bound leaves no value to subtract from.
const REMAINDER_UNKNOWN_REASON: &str = "space.data_insufficient";
fn bytes(size: &SizeValue) -> Result<u64, EngineError> {
    match &size.amount {
        SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => Ok(bytes.get()),
        _ => Err(internal()),
    }
}
fn quantity(unit: &str, value: u64) -> Result<VizQuantity, EngineError> {
    VizQuantity::exact(unit, i128::from(value)).ok_or_else(internal)
}
/// The sum of the member counts of every group shape of the model, nested groups
/// included: a lower bound as soon as one count is, and unknown when one count is.
fn grouped_items(shapes: &[VizShape]) -> Result<VizQuantity, EngineError> {
    let mut sum = 0_u64;
    let mut lower = false;
    for count in shapes
        .iter()
        .filter_map(|shape| shape.member_count.as_ref())
    {
        let value = match count.amount() {
            VizAmount::Exact { value } => value,
            VizAmount::LowerBound { value } => {
                lower = true;
                value
            }
            VizAmount::Unknown { reason_key } => {
                return VizQuantity::unknown("items", reason_key.clone()).ok_or_else(internal);
            }
        };
        let value = u64::try_from(value.get()).map_err(|_| internal())?;
        sum = sum.checked_add(value).ok_or_else(internal)?;
    }
    if lower {
        VizQuantity::lower_bound("items", sum).ok_or_else(internal)
    } else {
        quantity("items", sum)
    }
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
            crate::viz_pages::Error::Cancelled => "request.cancelled",
            _ => "support.internal_error",
        },
        vec![],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::visualization::model::{VizMatchKey, VizShape};

    fn shape(count: Option<VizQuantity>) -> Result<VizShape, EngineError> {
        Ok(VizShape {
            item_ref: None,
            group_ref: Some("g".into()),
            match_key: VizMatchKey::NotApplicable {},
            geometry: du_core_contracts::visualization::geometry::Geometry::new(
                "rect",
                vec![
                    du_core_contracts::visualization::geometry::Point { x: 0, y: 0 },
                    du_core_contracts::visualization::geometry::Point { x: 1, y: 1 },
                ],
            )
            .map_err(|_| internal())?,
            size: SizeValue {
                basis: du_core_contracts::SizeBasis::Logical,
                evidence: du_core_contracts::SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: 1_u64.into(),
                },
            },
            quantity: None,
            legend_class_key: "class".into(),
            display_flags: vec![],
            shape_ref: None,
            parent_shape_ref: None,
            visible_depth: None,
            root_branch_ref: None,
            member_count: count,
            item_kind: None,
            type_slices: None,
        })
    }

    #[test]
    fn the_grouped_total_adds_the_member_counts_and_keeps_what_is_not_exact()
    -> Result<(), EngineError> {
        let exact = |n| VizQuantity::exact("items", n);
        let sum = |counts: Vec<Option<VizQuantity>>| -> Result<VizQuantity, EngineError> {
            let shapes = counts
                .into_iter()
                .map(shape)
                .collect::<Result<Vec<_>, _>>()?;
            grouped_items(&shapes)
        };
        // No group is zero items, and a shape without a count (an item) adds nothing.
        assert_eq!(sum(vec![])?, exact(0).ok_or_else(internal)?);
        assert_eq!(
            sum(vec![exact(40), exact(60), None])?,
            exact(100).ok_or_else(internal)?
        );
        // One count that is a lower bound makes the sum a lower bound.
        assert_eq!(
            sum(vec![exact(40), VizQuantity::lower_bound("items", 60)])?,
            VizQuantity::lower_bound("items", 100).ok_or_else(internal)?
        );
        // An unknown count makes the sum unknown, with its reason, and is not read as zero.
        let unknown = VizQuantity::unknown("items", "viz.unavailable.no_attribution".into());
        assert_eq!(
            sum(vec![exact(40), unknown.clone()])?,
            unknown.ok_or_else(internal)?
        );
        Ok(())
    }

    #[test]
    fn a_table_only_fallback_publishes_the_no_attribution_key() {
        assert_eq!(
            super::fallback_reason(crate::viz_pages::render::FallbackReason::NoAttribution),
            crate::required_data::Reason::NoAttribution.key()
        );
        assert_eq!(
            super::fallback_reason(crate::viz_pages::render::FallbackReason::NoAttribution),
            "viz.unavailable.no_attribution"
        );
    }
}
