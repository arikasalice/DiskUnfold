//! Read space totals from the immutable version pinned at request admission.
pub(crate) mod detail;
pub(crate) mod volume_gap;
use std::sync::Arc;

use du_analysis_index::{
    IngestSink, QueryError, ResultVersion,
    query::{
        SpaceTotalError,
        page::{ResultSets, SearchReadError},
    },
};
use du_core_contracts::{
    ByteSize, Id, SizeAmount, SizeBasis, SizeValue,
    analysis_ipc::{SpaceTarget, SpaceTotal, SpaceTotalRequest, SpaceTotalSize},
    browse_page_ipc::QueryStrategy,
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead, SpaceAggregate},
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    source_exposure::{admit_version_source, pin_current_for, pin_latest_for},
};

/// Registers the GUI query against the same store used for scan publication.
pub fn register_total(builder: &mut EngineBuilder, store: IngestSink) {
    let queries = Arc::new(ResultSets::new(store.clone()));
    register_total_with_queries(builder, queries);
}

/// Registers space totals with the session registry used by the search producer.
pub fn register_total_with_queries(builder: &mut EngineBuilder, queries: Arc<ResultSets>) {
    builder.register(Total {
        store: queries.source(),
        queries,
    });
}

struct Total {
    store: IngestSink,
    queries: Arc<ResultSets>,
}

/// Serves `space.total` for a scope root, one folder or one admitted query result.
///
/// Every response names the aggregation rule of the pinned version in `sizeRule`. A scope
/// root and a folder also carry `cancelledUnreadFolders`, the count of cancelled-unread
/// folders at or below the target in that version (absent when the version does not provide
/// it). A query result is a set of rows without a single subtree, so it never carries
/// `cancelledUnreadFolders`.
impl UseCase for Total {
    const NAME: &'static str = "space.total";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SpaceTotalRequest;
    type Response = SpaceTotal;

    fn check_scope(&self, request: &SpaceTotalRequest, _: &CallerCtx) -> Result<(), EngineError> {
        // A folder reference must agree with an explicit result before either is looked up.
        if let (Some(version), SpaceTarget::Folder { item }) =
            (&request.result_version, &request.target)
            && version != &item.result_version
        {
            return Err(EngineError::reason("result.requery_required"));
        }
        Ok(())
    }

    fn execute(
        &self,
        request: SpaceTotalRequest,
        caller: &CallerCtx,
    ) -> Result<SpaceTotal, EngineError> {
        if let SpaceTarget::QueryResult { query_ref } = &request.target {
            let query = self
                .queries
                .open_search(query_ref, request.result_version.as_ref())
                .map_err(search_error)?;
            admit_version_source(query.version.as_ref(), caller)?;
            let aggregate =
                query
                    .space_total(|| caller.is_cancelled())
                    .map_err(|error| match error {
                        SpaceTotalError::Item(error) => query_error(error),
                        SpaceTotalError::Cancelled => EngineError::reason("support.internal_error"),
                    })?;
            // A matched set has no single subtree to count below, so the count stays absent.
            return Ok(response(&query.version, request.request_seq, &aggregate));
        }
        let version = pin(&self.store, request.result_version.as_ref(), caller.channel)?;
        total(&version, request)
    }
}

fn search_error(error: SearchReadError) -> EngineError {
    EngineError::reason(match error {
        SearchReadError::StaleVersion => "result.requery_required",
        SearchReadError::NotSearch => "space.target_out_of_scope",
        SearchReadError::Unavailable => "support.internal_error",
    })
}

/// Pins the requested result, or the latest one, for a caller on `channel`.
fn pin(
    store: &IngestSink,
    id: Option<&Id>,
    channel: Channel,
) -> Result<Arc<ResultVersion>, EngineError> {
    match id {
        Some(id) => pin_current_for(store, id, channel).map_err(|error| query_error(error.query())),
        None => pin_latest_for(store, channel)
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
    let (aggregate, cancelled_unread_folders) = match request.target {
        SpaceTarget::ScopeRoot {} => (
            version.scope_total(),
            version.cancelled_unread_folders_in_scope(),
        ),
        SpaceTarget::Folder { item } => {
            let detail = version.detail(&item).map_err(query_error)?;
            if detail.item.kind != ResultItemKind::Directory {
                return Err(EngineError::reason("space.target_out_of_scope"));
            }
            let aggregate = version
                .space(detail.item.item_no)
                .ok_or_else(|| EngineError::reason("space.target_out_of_scope"))?;
            let cancelled = version
                .cancelled_unread_folders(&item)
                .map_err(query_error)?;
            (aggregate, cancelled)
        }
        SpaceTarget::QueryResult { .. } => {
            return Err(EngineError::reason("space.target_out_of_scope"));
        }
    };
    Ok(SpaceTotal {
        cancelled_unread_folders,
        ..response(version, request.request_seq, aggregate)
    })
}

/// Converts one F03 aggregate to the published per-basis total.
///
/// Reports the size rule of `version` and leaves `cancelled_unread_folders` absent; a caller
/// whose target is a subtree sets it.
pub(crate) fn response(
    version: &ResultVersion,
    request_seq: u32,
    aggregate: &SpaceAggregate,
) -> SpaceTotal {
    SpaceTotal {
        result_version: version.version_id().clone(),
        request_seq,
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
        size_rule: Some(size_rule(version)),
        cancelled_unread_folders: None,
    }
}

fn size_rule(version: &ResultVersion) -> QueryStrategy {
    let rule = version.size_rule();
    QueryStrategy {
        id: rule.id.into(),
        revision: rule.revision,
    }
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
