//! Deferred single-parent ranked-bar publication for attributable exact sizes.
use super::*;
use crate::viz_pages::render;
use du_analysis_index::visualization::{
    bar_projection::{BarIdentity, ChildrenRequest},
    geometry::{Rect, Scale},
    typed_layout,
};
use du_core_contracts::job::JobOutcome;

pub(super) fn run(work: &compute::Compute, cancelled: &mut dyn FnMut() -> bool) -> JobOutcome {
    match prepare(work, cancelled) {
        Ok((candidate, model, count, group)) => {
            match (work.candidate.lock(), work.work.computed.lock()) {
                (Ok(mut slot), Ok(mut computed)) => {
                    *slot = Some(candidate);
                    computed.wire_model = model.map(Arc::new);
                    computed.ranked_group = group.map(|(group_ref, index)| {
                        (
                            work.admitted.source.result_version().clone(),
                            group_ref,
                            index,
                        )
                    });
                    let mut outcome = compute::outcome(true, None);
                    outcome.counts.completed = count;
                    outcome
                }
                _ => compute::outcome(false, Some("support.internal_error")),
            }
        }
        Err(_) => compute::outcome(false, (!cancelled()).then_some("support.internal_error")),
    }
}

fn prepare(
    work: &compute::Compute,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<RankedWork, EngineError> {
    let admitted = &work.admitted;
    let request = &admitted.request;
    let source = admitted.source.version();
    let mut evidence = HashMap::new();
    let mut presentations = Vec::new();
    let folder = du_analysis_index::browse::open(source, Some(&request.aggregate_scope_ref))
        .map_err(|_| internal())?;
    // Reserve one shape for Other only when this scope exceeds the limit.
    let top = top_with_other_slot(folder.children.len(), admitted.max_shapes);
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
    if matches!(
        admitted
            .required_data
            .as_ref()
            .map(|value| value.evaluation),
        Some(crate::required_data::Evaluation::Unavailable(_))
    ) {
        let candidate = work
            .session
            .prepare_ranked(
                &admitted.source,
                render::RankedInput {
                    parent: &request.aggregate_scope_ref,
                    basis: &request.size_basis,
                    fingerprint: admitted.key.clone(),
                    layout: None,
                    evidence: presentations,
                },
                cancelled,
            )
            .map_err(|_| internal())?;
        return Ok((
            Candidate::Projection(
                work.session
                    .defer_reserved(work.session.reserve_prepared(candidate)),
            ),
            None,
            evidence.len() as u64,
            None,
        ));
    }
    let result = work.session.compute_pinned_ranked_bar(
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
            top,
            max_bars: admitted.max_shapes,
        },
        &admitted.descriptor,
        cancelled,
    );
    let layout = match result {
        Ok(pinned)
            if pinned
                .selected()
                .output
                .bars
                .iter()
                .any(|bar| matches!(bar.identity, BarIdentity::Other { .. })) =>
        {
            return super::ranked_group_model::prepare(
                work,
                pinned,
                presentations,
                &evidence,
                cancelled,
            );
        }
        Ok(pinned) => {
            super::ranked_projection::to_projection(admitted, pinned.into_selected(), cancelled)?
        }
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
        Candidate::Projection(work.session.defer_reserved(reserved)),
        model,
        evidence.len() as u64,
        None,
    ))
}

/// Reserves one position for Other only when individual rows exceed the limit.
fn top_with_other_slot(children: usize, limit: usize) -> usize {
    if children > limit {
        limit.saturating_sub(1)
    } else {
        limit
    }
}

#[cfg(test)]
mod tests {
    use super::top_with_other_slot;

    #[test]
    fn other_slot_is_reserved_only_above_the_shape_limit() {
        assert_eq!(top_with_other_slot(2, 3), 3);
        assert_eq!(top_with_other_slot(3, 3), 3);
        assert_eq!(top_with_other_slot(4, 3), 2);
        assert_eq!(top_with_other_slot(2, 1), 0);
    }
}
