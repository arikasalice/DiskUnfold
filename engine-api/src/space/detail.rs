//! Read-only item space projection, separate from filesystem browse details.
use du_analysis_index::{
    IngestSink, ResultVersion,
    space_detail::{ExclusiveDecision, HardlinkSummary, SpaceDetail as ItemEvidence},
};
use du_core_contracts::{
    SizeAmount, SizeEvidence, SizeValue,
    browse_ipc::{BrowseDetailRequest, BrowseValue},
    result::SpaceAggregate,
    space_detail_ipc::{SpaceDetail, SpaceDetailSize, SpaceExclusiveDecision, SpaceLinkGroup},
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

const UNAVAILABLE: &str = "space.data_insufficient";

pub(crate) fn register(builder: &mut EngineBuilder, store: IngestSink) {
    builder.register(Query(store));
}

struct Query(IngestSink);
impl UseCase for Query {
    const NAME: &'static str = "space.detail";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = BrowseDetailRequest;
    type Response = SpaceDetail;

    fn check_scope(&self, request: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        if request.result_version != request.item.result_version {
            return Err(EngineError::reason("result.requery_required"));
        }
        Ok(())
    }

    fn execute(&self, request: Self::Request, _: &CallerCtx) -> Result<SpaceDetail, EngineError> {
        let version = super::pin(&self.0, Some(&request.result_version))?;
        let facts = version
            .space_detail(&request.item)
            .map_err(super::query_error)?;
        Ok(project(&version, facts))
    }
}

fn project(version: &ResultVersion, facts: ItemEvidence) -> SpaceDetail {
    let aggregate = version.space(facts.item.item_no);
    let (exclusive, shared, exclusive_decision) = exclusive_parts(&facts);
    SpaceDetail {
        item: facts.item,
        logical: size(facts.logical, aggregate),
        allocated: size(facts.allocated, aggregate),
        unique_physical: size(facts.unique_physical, aggregate),
        exclusive,
        shared,
        link_group: link_group(facts.links),
        placeholder_local_state: None,
        auxiliary: unknown("space.aux_not_included"),
        exclusive_decision,
        observed_at: unknown(UNAVAILABLE),
        capabilities: unknown(UNAVAILABLE),
    }
}

fn exclusive_parts(
    facts: &ItemEvidence,
) -> (
    BrowseValue<SpaceDetailSize>,
    BrowseValue<SpaceDetailSize>,
    SpaceExclusiveDecision,
) {
    let (exclusive, shared, reason) = match facts.exclusive {
        ExclusiveDecision::Confirmed => (
            size(facts.allocated.clone(), None),
            size(
                facts.allocated.clone().map(|value| SizeValue {
                    amount: SizeAmount::Exact { bytes: 0.into() },
                    ..value
                }),
                None,
            ),
            None,
        ),
        ExclusiveDecision::Unknown { reason_key } => (
            unknown(reason_key),
            unknown("space.sharing_unknown"),
            Some(reason_key),
        ),
        ExclusiveDecision::NotApplicable => {
            (BrowseValue::Absent, BrowseValue::Absent, Some(UNAVAILABLE))
        }
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
        return unknown("space.identity_unsupported");
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
