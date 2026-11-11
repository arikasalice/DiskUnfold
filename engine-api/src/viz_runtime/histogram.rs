//! Byte histogram work admitted by the live visualization runtime.
use super::*;
use crate::viz_pages::{histogram, production::RowPresentation};
use du_analysis_index::visualization::{
    geometry::{Rect, Scale},
    histogram::Basis,
    histogram_dispatch,
};
use du_core_contracts::{
    ItemRef,
    job::JobOutcome,
    visualization::{render::VizFallback, response::VizRenderModelResponse},
};

pub(super) fn run(work: &compute::Compute, cancelled: &mut dyn FnMut() -> bool) -> JobOutcome {
    let mut frozen = None;
    match prepare(work, cancelled, &mut frozen) {
        Ok((candidate, model, progress)) => {
            let deferred = match work.session.defer_histogram(candidate, cancelled) {
                Ok(value) => value,
                Err(_) => {
                    return unfinished(cancelled(), frozen);
                }
            };
            match (work.candidate.lock(), work.work.computed.lock()) {
                (Ok(mut slot), Ok(mut computed)) => {
                    *slot = Some(Candidate::Histogram(deferred));
                    computed.wire_model = model.map(Arc::new);
                    computed.histogram_revision =
                        Some(work.admitted.source.result_version().clone());
                    let mut outcome = compute::outcome(true, None);
                    outcome.counts.completed = progress.total;
                    outcome
                }
                _ => compute::outcome(false, Some("support.internal_error")),
            }
        }
        Err(_) => unfinished(cancelled(), frozen),
    }
}

fn unfinished(cancelled: bool, frozen: Option<FrozenProgress>) -> JobOutcome {
    let mut outcome = compute::outcome(false, (!cancelled).then_some("support.internal_error"));
    // No model rows have been finalized when private candidate preparation is
    // interrupted. Preserve the frozen observed scope when it is available.
    if cancelled && let Some(progress) = frozen {
        outcome.counts.unprocessed = progress.total;
    }
    outcome
}

fn prepare(
    work: &compute::Compute,
    cancelled: &mut dyn FnMut() -> bool,
    frozen: &mut Option<FrozenProgress>,
) -> Result<
    (
        histogram::SelectedCandidate,
        Option<VizModel>,
        FrozenProgress,
    ),
    EngineError,
> {
    let admitted = &work.admitted;
    let facts = admitted.histogram.as_ref().ok_or_else(internal)?;
    let source = admitted.source.version();
    let request = &admitted.request;
    let bounds = Rect {
        left: request.visible_window.left,
        top: request.visible_window.top,
        right: request.visible_window.right,
        bottom: request.visible_window.bottom,
    };
    let rendered = work
        .session
        .compute_histogram(
            source.clone(),
            histogram_dispatch::Request {
                version: source,
                scope: &request.aggregate_scope_ref,
                size_basis: request.size_basis.clone(),
                unit_table: facts.unit_table,
                unit_table_revision: facts.unit_table_revision,
                bounds,
                basis: Basis::Bytes,
                scale: Scale::Automatic,
                unknown_reason: &facts.unknown_reason,
            },
            cancelled,
        )
        .map_err(|_| internal())?;
    let bins = rendered.rendered().prepared().unit_bins().bins.clone();
    let progress = FrozenProgress::from_model(rendered.rendered().prepared().model())?;
    *frozen = Some(progress);
    let mut items = Vec::new();
    for item_no in rendered
        .rendered()
        .prepared()
        .model()
        .histogram
        .bins
        .iter()
        .flat_map(|bin| &bin.members)
    {
        if cancelled() {
            return Err(internal());
        }
        let item = ItemRef {
            result_version: admitted.source.result_version().clone(),
            item_no: *item_no,
        };
        let detail = source.detail(&item).map_err(|_| internal())?;
        let mut evidence = work
            .producer
            .child(&admitted.source, detail.item, request)?;
        if !admitted
            .facts
            .classes
            .contains_key(&evidence.presentation.legend_class_key)
        {
            return Err(internal());
        }
        evidence.presentation.display_flags.sort_unstable();
        evidence.presentation.display_flags.dedup();
        items.push(RowPresentation {
            item,
            legend_class_key: evidence.presentation.legend_class_key,
            display_flags: evidence.presentation.display_flags,
        });
    }
    let candidate = histogram::prepare_selected(
        histogram::Request {
            version: source.clone(),
            scope: &request.aggregate_scope_ref,
            size_basis: request.size_basis.clone(),
            bins: &bins,
            bounds,
            scale: Scale::Automatic,
            fingerprint: admitted.key.clone(),
            bin_presentation: facts.bins.clone(),
            item_presentation: items,
            unknown_reason: facts.unknown_reason.clone(),
        },
        rendered,
        cancelled,
    )
    .map_err(|_| internal())?;
    let model = if candidate.no_attribution() {
        None
    } else {
        Some(super::histogram_model::build(
            admitted, &candidate, cancelled,
        )?)
    };
    Ok((candidate, model, progress))
}

/// The exact number of observed files in the pinned scope, including files
/// whose size cannot be assigned to a finite histogram interval. This is an
/// item count, not a count of bins, shapes, bytes, or unobserved source gaps.
#[derive(Clone, Copy)]
struct FrozenProgress {
    total: u64,
}

impl FrozenProgress {
    fn from_model(
        model: &du_analysis_index::visualization::histogram_projection::Model,
    ) -> Result<Self, EngineError> {
        let histogram = &model.histogram;
        let mut observed =
            u64::try_from(histogram.uncertain_items.len()).map_err(|_| internal())?;
        observed = observed
            .checked_add(u64::try_from(histogram.unclassified_items.len()).map_err(|_| internal())?)
            .ok_or_else(internal)?;
        for bin in &histogram.bins {
            observed = observed
                .checked_add(u64::try_from(bin.members.len()).map_err(|_| internal())?)
                .ok_or_else(internal)?;
        }
        if observed != model.summary.counts.files || observed != histogram.observed_items {
            return Err(internal());
        }
        Ok(Self { total: observed })
    }
}

pub(super) fn response(
    session: &Session,
    admitted: &Admitted,
    fallback: &VizFallback,
    no_attribution: bool,
    model: Option<&VizModel>,
) -> Result<VizRenderResponse, EngineError> {
    if no_attribution {
        return envelope::response(
            session,
            admitted,
            &CommittedRender::Fallback {
                reason: crate::viz_pages::render::FallbackReason::NoAttribution,
                fallback: fallback.clone(),
            },
            None,
        );
    }
    session
        .rows(&du_core_contracts::visualization::rows::VizRowsRequest {
            model_ref: fallback.model_ref.clone(),
            result_revision: fallback.result_revision.clone(),
            start: 0,
            count: 1,
        })
        .map_err(|_| internal())?;
    let mut model = model.ok_or_else(internal)?.clone();
    model.model_ref.clone_from(&fallback.model_ref);
    let result = VizRenderResponse::Model(VizRenderModelResponse::Model {
        request_seq: admitted.request.request_seq,
        result_revision: fallback.result_revision.clone(),
        observed_at: admitted.source.version().observed_at().cloned(),
        completeness: admitted.source.version().completeness(),
        model: Box::new(model),
    });
    result.validate().map_err(|_| internal())?;
    Ok(result)
}
