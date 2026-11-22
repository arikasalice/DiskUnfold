//! Read-only item space projection, separate from filesystem browse details.
use du_analysis_index::{
    IngestSink, ResultVersion,
    space_detail::{ExclusiveDecision, HardlinkSummary, SharedSpace, SpaceDetail as ItemEvidence},
};
use du_core_contracts::{
    SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    browse_ipc::BrowseValue,
    result::SpaceAggregate,
    space_detail_ipc::{
        SpaceDetail, SpaceDetailRequest, SpaceDetailSize, SpaceExclusiveDecision, SpaceLinkGroup,
    },
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

const UNAVAILABLE: &str = "space.data_insufficient";
/// A placeholder whose local occupancy the OS does not report (F03 FR6.HD1).
const PLACEHOLDER_LOCAL_UNKNOWN: &str = "space.placeholder_local_unknown";

pub(crate) fn register(builder: &mut EngineBuilder, store: IngestSink) {
    builder.register(Query(store));
}

struct Query(IngestSink);
impl UseCase for Query {
    const NAME: &'static str = "space.detail";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SpaceDetailRequest;
    type Response = SpaceDetail;

    fn check_scope(&self, request: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        if request.result_version != request.item.result_version {
            return Err(EngineError::reason("result.requery_required"));
        }
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<SpaceDetail, EngineError> {
        let version = super::pin(&self.0, Some(&request.result_version), caller.channel)?;
        let facts = version
            .space_detail(&request.item)
            .map_err(super::query_error)?;
        project(&version, facts)
    }
}

fn project(version: &ResultVersion, facts: ItemEvidence) -> Result<SpaceDetail, EngineError> {
    let aggregate = version.space(facts.item.item_no);
    let observed_at = version.item_observed_at(facts.item.item_no).map_or_else(
        || unknown(UNAVAILABLE),
        |value| BrowseValue::Present {
            value: value.clone(),
        },
    );
    let (exclusive, shared, exclusive_decision) = exclusive_parts(&facts);
    // The allocation of such a placeholder is unknown for this cause, never zero.
    let placeholder_unknown = version
        .item_placeholder_observation(&facts.item)
        .ok()
        .flatten()
        .is_some_and(|value| value.local_occupancy_unknown());
    let physical = |value, aggregate| {
        if placeholder_unknown {
            unknown(PLACEHOLDER_LOCAL_UNKNOWN)
        } else {
            size(value, aggregate)
        }
    };
    let folder_flags = crate::browse::folder::collection::detail_flags(version, &facts.item)?;
    Ok(SpaceDetail {
        item: facts.item,
        logical: size(facts.logical, aggregate.as_ref()),
        allocated: physical(facts.allocated, aggregate.as_ref()),
        unique_physical: physical(facts.unique_physical, aggregate.as_ref()),
        exclusive,
        shared,
        link_group: link_group(facts.links),
        placeholder_local_state: placeholder_unknown.then(|| unknown(PLACEHOLDER_LOCAL_UNKNOWN)),
        auxiliary: unknown("space.aux_not_included"),
        exclusive_decision,
        observed_at,
        capabilities: unknown(UNAVAILABLE),
        data_source: Some(du_core_contracts::result::ResultVersionRead::data_source(
            version,
        )),
        folder_flags,
    })
}

fn exclusive_parts(
    facts: &ItemEvidence,
) -> (
    BrowseValue<SpaceDetailSize>,
    BrowseValue<SpaceDetailSize>,
    SpaceExclusiveDecision,
) {
    let (exclusive, reason) = match facts.exclusive {
        ExclusiveDecision::Confirmed => (size(facts.allocated.clone(), None), None),
        ExclusiveDecision::Unknown { reason_key } => (unknown(reason_key), Some(reason_key)),
        ExclusiveDecision::NotApplicable => (BrowseValue::Absent, Some(UNAVAILABLE)),
    };
    let shared = match facts.shared {
        SharedSpace::ConfirmedZero => size(
            Some(SizeValue {
                basis: SizeBasis::Allocated,
                evidence: SizeEvidence::OsReported,
                amount: SizeAmount::Exact { bytes: 0.into() },
            }),
            None,
        ),
        // Sharing evidence is not collected yet (DLD FR11.HD1.app-host.DD1).
        SharedSpace::Unknown => unknown("space.sharing_not_collected"),
        SharedSpace::NotApplicable => BrowseValue::Absent,
    };
    (
        exclusive,
        shared,
        SpaceExclusiveDecision {
            confirmed: reason.is_none(),
            reason_key: reason.map(str::to_owned),
        },
    )
}

fn size(
    value: Option<SizeValue>,
    aggregate: Option<&SpaceAggregate>,
) -> BrowseValue<SpaceDetailSize> {
    let Some(size) = value.filter(|value| {
        value.amount != SizeAmount::Unknown && value.evidence != SizeEvidence::Unknown
    }) else {
        return unknown(UNAVAILABLE);
    };
    let measurement_limits = aggregate.map_or_else(Vec::new, |aggregate| {
        super::measurement_limits(&size, &aggregate.limitations)
    });
    BrowseValue::Present {
        value: SpaceDetailSize {
            size,
            measurement_limits,
        },
    }
}

fn link_group(links: Option<HardlinkSummary>) -> BrowseValue<SpaceLinkGroup> {
    let Some(links) = links else {
        return BrowseValue::Absent;
    };
    let Some(paths) = links.in_scope else {
        return unknown(
            links
                .lookup_unavailable_reason
                .unwrap_or("space.identity_unsupported"),
        );
    };
    BrowseValue::Present {
        value: SpaceLinkGroup {
            reported_link_count: links.observed_link_count.map(|count| count.to_string()),
            in_scope_paths: paths,
            has_outside_links: links.outside_scope,
        },
    }
}

fn unknown<T>(reason: &str) -> BrowseValue<T> {
    BrowseValue::Unknown {
        reason_key: reason.into(),
    }
}

#[cfg(test)]
mod tests;
