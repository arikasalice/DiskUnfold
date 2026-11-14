//! `group.summary` and `group.cancel`, and the published summary of a classification.
use std::sync::Arc;

use du_core_contracts::{
    Id,
    browse_page_ipc::{QueryStrategy, ResultOrigin},
    group_ipc::{
        GroupCancelOutcome, GroupCancelRequest, GroupCancelResponse, GroupClassifyResponse,
        GroupReasonCount, GroupSummary, GroupSummaryRequest, GroupSummaryRow,
    },
    result::ResultVersionRead,
};

use super::{JOB_KIND, Services, channels, codec, tables};
use crate::{
    api::{CallerCtx, Channel, UseCase},
    error::EngineError,
};

pub(super) struct Summary(pub(super) Arc<Services>);

impl UseCase for Summary {
    const NAME: &'static str = "group.summary";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = GroupSummaryRequest;
    type Response = GroupSummary;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, request: Self::Request, _: &CallerCtx) -> Result<GroupSummary, EngineError> {
        build_summary(&self.0, &request.classification_ref)
    }
}

pub(super) fn summary_response(
    services: &Services,
    seq: u32,
    reference: &Id,
) -> Result<GroupClassifyResponse, EngineError> {
    Ok(GroupClassifyResponse::Summary {
        request_seq: seq,
        summary: Box::new(build_summary(services, reference)?),
    })
}

/// Builds the published summary of a registered classification.
fn build_summary(services: &Services, reference: &Id) -> Result<GroupSummary, EngineError> {
    let (classification, meta) = services
        .registered
        .get(reference, &services.assembly.store)?;
    let current_ruleset = (meta.ruleset_version != 0)
        .then(|| codec::load(services.rules.as_ref()).ok())
        .flatten()
        .map(|loaded| loaded.ruleset.version_no);
    let limitations = tables::limitations(&tables::Evidence {
        classification: &classification,
        meta: &meta,
        current_ruleset,
    });
    let summary = classification.summary();
    let version = classification.version();
    let unavailable = classification.rule_unavailable();
    let groups = meta
        .group_ids
        .iter()
        .zip(&meta.names)
        .zip(&summary.groups)
        .enumerate()
        .map(|(index, ((group_id, name), counts))| GroupSummaryRow {
            group_id: group_id.clone(),
            display_name: name.clone(),
            display_order: u32::try_from(index).unwrap_or(u32::MAX),
            member_count: counts.members,
            overlap_member_count: counts.overlap_members,
            undecidable_count: counts.undecidable,
            rule_unevaluable: unavailable.get(index).copied().unwrap_or(false),
        })
        .collect();
    Ok(GroupSummary {
        classification_ref: reference.clone(),
        result_version: version.version_id().clone(),
        origin: ResultOrigin(version.origin().into()),
        data_source: Some(version.data_source()),
        observed_at: version.observed_at().cloned(),
        completeness: version.completeness(),
        scope: classification.root().cloned(),
        ruleset_version: meta.ruleset_version,
        ruleset_fingerprint: meta.fingerprint.clone(),
        reference_time: None,
        groups,
        scope_file_count: summary.scope_files,
        matched_count: summary.matched,
        overlap_count: summary.overlap,
        unclassified_count: summary.unclassified,
        undecidable_count: summary.undecidable,
        reconciled: summary.reconciled,
        undecidable_reasons: summary
            .reason_counts
            .iter()
            .map(|(reason_key, count)| GroupReasonCount {
                reason_key: (*reason_key).into(),
                count: *count,
            })
            .collect(),
        limitations,
        predicates: classification
            .predicates()
            .into_iter()
            .map(|(id, revision)| QueryStrategy { id, revision })
            .collect(),
    })
}

pub(super) struct Cancel(pub(super) Arc<Services>);

impl UseCase for Cancel {
    const NAME: &'static str = "group.cancel";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = GroupCancelRequest;
    type Response = GroupCancelResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<GroupCancelResponse, EngineError> {
        let runtime = &self.0.assembly.runtime;
        let outcome = match request {
            GroupCancelRequest::View {
                view_ref,
                request_seq,
            } => self.0.channels.cancel(&view_ref, request_seq, runtime)?,
            GroupCancelRequest::Job { job_ref } => {
                let owned = runtime
                    .snapshot(&job_ref)
                    .is_ok_and(|snapshot| snapshot.kind_id == JOB_KIND);
                if owned {
                    channels::outcome(runtime.cancel(&job_ref).ok())
                } else {
                    GroupCancelOutcome::NoEffect
                }
            }
        };
        Ok(GroupCancelResponse { outcome })
    }
}
