//! Read space totals from the immutable version pinned at request admission.
use du_analysis_index::{IngestSink, ResultVersion};
use du_core_contracts::{
    ByteSize, SizeAmount, SizeBasis, SizeValue,
    analysis_ipc::{SpaceTarget, SpaceTotal, SpaceTotalRequest, SpaceTotalSize},
    result::{ResultCompleteness, ResultVersionRead},
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
        let version = self
            .0
            .pin()
            .map_err(|_| EngineError::reason("support.internal_error"))?
            .ok_or_else(|| EngineError::reason("ref.unavailable"))?;
        if request
            .result_version
            .as_ref()
            .is_some_and(|id| id != version.version_id())
        {
            return Err(EngineError::reason("result.requery_required"));
        }
        total(&version, request)
    }
}

fn total(version: &ResultVersion, request: SpaceTotalRequest) -> Result<SpaceTotal, EngineError> {
    let item_no = match request.target {
        SpaceTarget::ScopeRoot {} => 0,
        // The current index cannot yet distinguish folders from skipped links.
        // An item number alone is insufficient evidence for a folder query.
        _ => return Err(EngineError::reason("space.target_out_of_scope")),
    };
    let aggregate = version
        .space(item_no)
        .ok_or_else(|| EngineError::reason("space.target_out_of_scope"))?;
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
        inaccessible_ranges: None,
        untraversed_links: None,
    })
}

fn basis(size: &SizeValue, completeness: ResultCompleteness, limits: &[&str]) -> SpaceTotalSize {
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
    SpaceTotalSize {
        size: size.clone(),
        completeness,
        measurement_limits,
    }
}
