//! Read space totals from the immutable version pinned at request admission.
pub(crate) mod detail;
pub(crate) mod volume_gap;
use std::sync::Arc;

use du_analysis_index::{IngestSink, QueryError, ResultVersion};
use du_core_contracts::{
    ByteSize, Id, SizeAmount, SizeBasis, SizeValue,
    analysis_ipc::{SpaceTarget, SpaceTotal, SpaceTotalRequest, SpaceTotalSize},
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

/// Registers the GUI query against the same store used for scan publication.
pub fn register_total(builder: &mut EngineBuilder, store: IngestSink) {
    builder.register(Total(store));
}

struct Total(IngestSink);

impl UseCase for Total {
    const NAME: &'static str = "space.total";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SpaceTotalRequest;
    type Response = SpaceTotal;

    fn check_scope(&self, _: &SpaceTotalRequest, _: &CallerCtx) -> Result<(), EngineError> {
        // This query has no OS path inputs. The target is checked against the pinned version.
        Ok(())
    }

    fn execute(
        &self,
        request: SpaceTotalRequest,
        _: &CallerCtx,
    ) -> Result<SpaceTotal, EngineError> {
        let version = pin(&self.0, request.result_version.as_ref())?;
        total(&version, request)
    }
}

fn pin(store: &IngestSink, id: Option<&Id>) -> Result<Arc<ResultVersion>, EngineError> {
    match id {
        Some(id) => store.pin_current(id).map_err(query_error),
        None => store
            .pin()
            .map_err(|_| EngineError::reason("support.internal_error"))?
            .ok_or_else(|| EngineError::reason("ref.unavailable")),
    }
}

fn query_error(error: QueryError) -> EngineError {
    EngineError::reason(match error {
        QueryError::NotFound | QueryError::PathUnavailable => "ref.unavailable",
        QueryError::StaleVersion => "result.requery_required",
        QueryError::InvalidPath | QueryError::NotDirectory | QueryError::KindUnknown => {
            "space.target_out_of_scope"
        }
        QueryError::Unavailable => "support.internal_error",
    })
}

fn total(version: &ResultVersion, request: SpaceTotalRequest) -> Result<SpaceTotal, EngineError> {
    let aggregate = match request.target {
        SpaceTarget::ScopeRoot {} => version.scope_total(),
        SpaceTarget::Folder { item } => {
            let detail = version.detail(&item).map_err(query_error)?;
            if detail.item.kind != ResultItemKind::Directory {
                return Err(EngineError::reason("space.target_out_of_scope"));
            }
            version
                .space(detail.item.item_no)
                .ok_or_else(|| EngineError::reason("space.target_out_of_scope"))?
        }
        SpaceTarget::QueryResult { .. } => {
            return Err(EngineError::reason("space.target_out_of_scope"));
        }
    };
    Ok(SpaceTotal {
        result_version: version.version_id().clone(),
        request_seq: request.request_seq,
        sizes: vec![
            basis(
                &aggregate.logical,
                aggregate.completeness,
                &aggregate.limitations,
            ),
            basis(
                &aggregate.allocated,
                aggregate.allocated_completeness,
                &aggregate.limitations,
            ),
            basis(
                &aggregate.unique_physical,
                aggregate.unique_physical_completeness,
                &aggregate.limitations,
            ),
        ],
        completeness: version.completeness(),
        missing_items: Some(ByteSize::from(aggregate.missing_items)),
        inaccessible_ranges: aggregate.inaccessible_ranges.map(ByteSize::from),
        untraversed_links: Some(ByteSize::from(aggregate.untraversed_links)),
    })
}

fn basis(size: &SizeValue, completeness: ResultCompleteness, limits: &[&str]) -> SpaceTotalSize {
    SpaceTotalSize {
        size: size.clone(),
        completeness,
        measurement_limits: measurement_limits(size, limits),
    }
}

fn measurement_limits(size: &SizeValue, limits: &[&str]) -> Vec<String> {
    let mut measurement_limits: Vec<_> = limits
        .iter()
        .filter(|&&key| match key {
            "space.sharing_not_collected" => size.basis == SizeBasis::UniquePhysical,
            "space.allocated_not_collected"
            | "space.hardlink_identity_unknown"
            | "space.dedup_capacity" => size.basis != SizeBasis::Logical,
            _ => true,
        })
        .map(|key| (*key).to_owned())
        .collect();
    if size.amount == SizeAmount::Unknown {
        measurement_limits.push("space.data_insufficient".into());
    }
    measurement_limits
}
