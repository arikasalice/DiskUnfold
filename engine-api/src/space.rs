//! Read space totals from the immutable version pinned at request admission.
pub(crate) mod detail;
pub(crate) mod volume_gap;
use std::sync::Arc;

use du_analysis_index::{
    IngestSink, ObservationRange, QueryError, ResultVersion,
    query::{
        SpaceTotalError,
        page::{ResultSets, SearchReadError},
    },
};
use du_core_contracts::{
    ByteSize, Id, SizeAmount, SizeBasis, SizeValue,
    analysis_ipc::{
        SpaceObservationRange, SpaceTarget, SpaceTotal, SpaceTotalRequest, SpaceTotalSize,
    },
    browse_page_ipc::QueryStrategy,
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead, SpaceAggregate},
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    source_exposure::{admit_version_source, pin_current_for, pin_latest_for},
};

/// Registers the GUI query against the same store used for scan publication.
/// Measurement limit of an allocated total over volumes whose files may share extents:
/// the sum can exceed the real usage (#351).
const ALLOCATED_MAY_INCLUDE_SHARED: &str = "space.allocated_may_include_shared";

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
/// root and a folder carry `cancelledUnreadFolders` only when the exact count is known: a
/// sealed real scan with `Complete` or `Missing` status and no cancelled marks reports zero.
/// A mark alone does not prove total coverage, and a mark in a complete or missing result is
/// contradictory, so those counts are absent. Replacements do not establish coverage. A query
/// result is a set of rows without a single subtree, so it never carries `cancelledUnreadFolders`.
///
/// Every target carries `observationRange` from the same pinned version: the scope range, the
/// folder's subtree range, or the range of the pinned items the query total reads. Another
/// version or a cancellation returns no body at all.
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
            let total = query
                .space_total_with_observation_range(|| caller.is_cancelled())
                .map_err(|error| match error {
                    SpaceTotalError::Item(error) => query_error(error),
                    SpaceTotalError::Cancelled => EngineError::reason("request.cancelled"),
                })?;
            // A matched set has no single subtree to count below, so the count stays absent.
            return Ok(SpaceTotal {
                observation_range: Some(wire_range(total.observation_range)?),
                ..response(&query.version, request.request_seq, &total.aggregate)
            });
        }
        let version = pin(&self.store, request.result_version.as_ref(), caller.channel)?;
        let body = total(&version, request)?;
        // A cancelled request sends no total, like the query path above.
        if caller.is_cancelled() {
            return Err(EngineError::reason("support.internal_error"));
        }
        Ok(body)
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
    let (aggregate, cancelled_unread_folders, range) = match request.target {
        SpaceTarget::ScopeRoot {} => (
            version.scope_total().clone(),
            version.cancelled_unread_folders_in_scope(),
            version.scope_observation_range(),
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
            let range = version
                .observation_range(detail.item.item_no)
                .ok_or_else(|| EngineError::reason("space.target_out_of_scope"))?;
            (aggregate, cancelled, range)
        }
        SpaceTarget::QueryResult { .. } => {
            return Err(EngineError::reason("space.target_out_of_scope"));
        }
    };
    Ok(SpaceTotal {
        cancelled_unread_folders,
        observation_range: Some(wire_range(range)?),
        ..response(version, request.request_seq, &aggregate)
    })
}

/// Copies the pinned observation bounds into the wire range without other times.
fn wire_range(range: ObservationRange<'_>) -> Result<SpaceObservationRange, EngineError> {
    SpaceObservationRange::new(range.min.cloned(), range.max.cloned(), range.complete)
        .map_err(|_| EngineError::reason("support.internal_error"))
}

/// Converts one F03 aggregate to the published per-basis total.
///
/// Reports the size rule of `version` and leaves `cancelled_unread_folders` and
/// `observation_range` absent; a `space.total` caller sets them for its target. A group
/// total reuses this body without an observation range.
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
        observation_range: None,
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

/// The measurement limits `space.total` gives one item in `basis` (#351), or none.
///
/// The visualization's scope total reads them here, so the graph and the result summary
/// never disagree about the same item and basis.
pub(crate) fn item_limits(
    version: &ResultVersion,
    item_no: u32,
    basis: &SizeBasis,
) -> Option<Vec<String>> {
    let aggregate = version.space(item_no)?;
    let size = match basis {
        SizeBasis::Logical => &aggregate.logical,
        SizeBasis::Allocated => &aggregate.allocated,
        SizeBasis::UniquePhysical => &aggregate.unique_physical,
    };
    let limits = measurement_limits(size, &aggregate.limitations);
    (!limits.is_empty()).then_some(limits)
}

fn measurement_limits(size: &SizeValue, limits: &[&str]) -> Vec<String> {
    let mut measurement_limits: Vec<_> = limits
        .iter()
        .filter(|&&key| match key {
            "space.sharing_not_collected" => size.basis == SizeBasis::UniquePhysical,
            "space.allocated_not_collected"
            | "space.hardlink_identity_unknown"
            | "space.placeholder_local_unknown"
            | "space.dedup_capacity" => size.basis != SizeBasis::Logical,
            _ => true,
        })
        .map(|key| (*key).to_owned())
        .collect();
    // A sum of per-file allocations counts the extents that files share more than
    // once, so it can exceed the real usage (#351). Only this basis says so.
    if size.basis == SizeBasis::Allocated && limits.contains(&"space.sharing_not_collected") {
        measurement_limits.push(ALLOCATED_MAY_INCLUDE_SHARED.into());
    }
    if size.amount == SizeAmount::Unknown {
        measurement_limits.push("space.data_insufficient".into());
    }
    measurement_limits
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{SizeEvidence, SizeValue};

    fn size(basis: SizeBasis) -> SizeValue {
        SizeValue {
            basis,
            evidence: SizeEvidence::OsReported,
            amount: SizeAmount::Exact {
                bytes: ByteSize::from(1),
            },
        }
    }

    /// #351: only an allocated total over files that may share extents says it can
    /// exceed the real usage.
    #[test]
    fn only_the_allocated_basis_names_the_shared_space_limit() {
        let shared = ["space.sharing_not_collected"];
        let has = |basis, limits: &[&str]| {
            measurement_limits(&size(basis), limits)
                .contains(&ALLOCATED_MAY_INCLUDE_SHARED.to_owned())
        };
        assert!(has(SizeBasis::Allocated, &shared));
        assert!(!has(SizeBasis::Logical, &shared));
        assert!(!has(SizeBasis::UniquePhysical, &shared));
        // A volume that cannot share extents never says it.
        assert!(!has(SizeBasis::Allocated, &["space.aux_not_included"]));
    }

    /// #351: a sample result's scope total names the same limits as its `space.total`;
    /// the visualization reads them through the same function, for every basis.
    #[test]
    fn item_limits_are_the_limits_space_total_gives_a_sample() -> Result<(), String> {
        use du_analysis_index::{Observation, SampleSessionKey};
        use du_core_contracts::{
            Id, PathBytes, PathEncoding,
            result::{ResultNativePath, ResultScopeRoot},
        };
        let debug = |error: &dyn std::fmt::Debug| format!("{error:?}");
        let sink = IngestSink::default();
        let root = ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/sample"),
            },
            volume_id: vec![1],
            file_id: vec![2],
        };
        let mut build = sink
            .open_sample_forest(
                Id::from_v7_bytes([1; 16]),
                vec![root],
                &SampleSessionKey::new(Id::from_v7_bytes([9; 16])),
            )
            .map_err(|e| debug(&e))?;
        build
            .add_directory_at(0, Vec::new())
            .map_err(|e| debug(&e))?;
        build
            .add_at(
                0,
                Observation {
                    path: vec![b"a.pdf".to_vec()],
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact {
                            bytes: ByteSize::from(5),
                        },
                    }),
                },
            )
            .map_err(|e| debug(&e))?;
        let version = build
            .seal(Id::from_v7_bytes([2; 16]), ResultCompleteness::Complete)
            .map_err(|e| debug(&e))?;
        let aggregate = version.space(0).ok_or("no aggregate")?;
        let total = response(&version, 1, &aggregate);
        for size in &total.sizes {
            let limits = item_limits(&version, 0, &size.size.basis).unwrap_or_default();
            assert_eq!(limits, size.measurement_limits, "{:?}", size.size.basis);
        }
        Ok(())
    }
}
