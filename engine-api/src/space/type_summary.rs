//! The legend classes of a whole result for the summary card (F02 #300).
use du_analysis_index::{
    IngestSink, ResultVersion,
    visualization::{folder_types::group_types, type_legend},
};
use du_core_contracts::{
    ItemRef,
    space_type_summary::{SpaceTypeSummary, SpaceTypeSummaryRequest},
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    viz_pages::type_slices,
};

#[cfg(test)]
mod tests;

pub(crate) fn register(builder: &mut EngineBuilder, store: IngestSink) {
    builder.register(TypeSummary(store));
}

struct TypeSummary(IngestSink);

/// Serves `space.type_summary` from the pinned result.
///
/// The classes of every scope root are counted in one walk by the code that counts a
/// folder's slices, so the slices equal the legend of the space distribution and a
/// file reached from two roots adds its allocation once. A sample result has no
/// classes and is refused like any other target outside the scope.
impl UseCase for TypeSummary {
    const NAME: &'static str = "space.type_summary";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SpaceTypeSummaryRequest;
    type Response = SpaceTypeSummary;

    fn check_scope(&self, _: &SpaceTypeSummaryRequest, _: &CallerCtx) -> Result<(), EngineError> {
        // The only source is a result version in this local GUI's store.
        Ok(())
    }

    fn execute(
        &self,
        request: SpaceTypeSummaryRequest,
        caller: &CallerCtx,
    ) -> Result<SpaceTypeSummary, EngineError> {
        let version = super::pin(&self.0, Some(&request.result_version), caller.channel)?;
        summary(&version, request, caller)
    }
}

fn summary(
    version: &ResultVersion,
    request: SpaceTypeSummaryRequest,
    caller: &CallerCtx,
) -> Result<SpaceTypeSummary, EngineError> {
    let roots: Vec<ItemRef> = version
        .scope_roots()
        .iter()
        .map(|root| ItemRef {
            result_version: request.result_version.clone(),
            item_no: root.item_no,
        })
        .collect();
    let types = match group_types(version, &roots, &request.basis, &mut || {
        caller.is_cancelled()
    }) {
        Ok(Some(types)) => types,
        Ok(None) => return Err(EngineError::reason("space.target_out_of_scope")),
        Err(type_legend::TotalsError::Cancelled) => {
            return Err(EngineError::reason("request.cancelled"));
        }
        Err(type_legend::TotalsError::InvalidScope) => {
            return Err(EngineError::reason("support.internal_error"));
        }
    };
    let lower_bound = types.below_unread;
    let slices =
        type_slices::to_slices(types).map_err(|_| EngineError::reason("support.internal_error"))?;
    let body = SpaceTypeSummary {
        result_version: request.result_version,
        basis: request.basis,
        slices,
        legend_order: type_legend::LISTED
            .iter()
            .map(|class| class.key.to_owned())
            .collect(),
        lower_bound,
    };
    body.validate()
        .map_err(|_| EngineError::reason("support.internal_error"))?;
    Ok(body)
}
