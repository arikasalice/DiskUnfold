//! `group.members`, `group.explain` and `group.totals` on one registered classification.
use std::sync::Arc;

use du_analysis_index::{
    group::{self, Category, Classification, Decision},
    query::{Basis, Sort},
};
use du_core_contracts::{
    Id, SizeBasis,
    browse_open_ipc::BrowseSortKey,
    browse_page_ipc::QueryStrategy,
    group_ipc::{
        GroupCategory, GroupConditionNode, GroupDecision, GroupExplainRequest, GroupExplanation,
        GroupMemberRow, GroupMembers, GroupMembersRequest, GroupRuleDecision, GroupTotal,
        GroupTotalRow, GroupTotals, GroupTotalsRequest,
    },
    result::{ResultVersionRead, SpaceAggregate},
};

use super::{Meta, Services, internal, tables};
use crate::{
    api::{CallerCtx, Channel, UseCase},
    error::EngineError,
    search_page, space,
};

/// Rows per member read (F36 DLD gui-web table 3).
const PAGE_LIMIT: u32 = 1_000;

fn default_sort() -> BrowseSortKey {
    BrowseSortKey {
        id: "sort.size".into(),
        revision: 1,
        descending: true,
        size_basis: Some(SizeBasis::Logical),
    }
}

fn group_index(meta: &Meta, id: &Id) -> Result<usize, EngineError> {
    meta.group_ids
        .iter()
        .position(|known| known == id)
        .ok_or_else(|| EngineError::reason("group.unknown"))
}

fn same_version(classification: &Classification, version: &Id) -> Result<(), EngineError> {
    if classification.version().version_id() == version {
        Ok(())
    } else {
        Err(EngineError::reason("cursor.invalid"))
    }
}

fn read_error(error: group::Error) -> EngineError {
    match error {
        group::Error::InvalidItem => EngineError::reason("ref.unavailable"),
        group::Error::InvalidRule(_) => EngineError::reason("sort.key_unknown"),
        _ => internal(),
    }
}

pub(super) struct Members(pub(super) Arc<Services>);

impl UseCase for Members {
    const NAME: &'static str = "group.members";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = GroupMembersRequest;
    type Response = GroupMembers;

    fn check_scope(&self, request: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        if request.count == 0 || request.count > PAGE_LIMIT {
            return Err(EngineError::reason("cursor.limit_exceeded"));
        }
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<GroupMembers, EngineError> {
        let services = &self.0;
        let (classification, meta) = services
            .registered
            .get(&request.classification_ref, &services.assembly.store)?;
        same_version(&classification, &request.result_version)?;
        let category = match &request.category {
            GroupCategory::Group { group_id } => Category::Group(group_index(&meta, group_id)?),
            GroupCategory::Overlap {} => Category::Overlap,
            GroupCategory::Unclassified {} => Category::Unclassified,
            GroupCategory::Undecidable {} => Category::Undecidable,
        };
        let key = request.sort.unwrap_or_else(default_sort);
        let basis = match key.size_basis {
            None | Some(SizeBasis::Logical) => Basis::Logical,
            Some(SizeBasis::Allocated) => Basis::Allocated,
            Some(SizeBasis::UniquePhysical) => {
                return Err(EngineError::reason("command.body_invalid"));
            }
        };
        let sorts = [Sort {
            id: &key.id,
            revision: key.revision,
            basis,
            descending: key.descending,
        }];
        let page = classification
            .page(
                category,
                &sorts,
                &services.registry,
                request.start as usize,
                request.count as usize,
                || caller.is_cancelled(),
            )
            .map_err(read_error)?;
        let version = classification.version();
        let mut rows = Vec::with_capacity(page.rows.len());
        for row in page.rows {
            let projected = search_page::row(version, &row.item).map_err(|_| internal())?;
            rows.push(GroupMemberRow {
                item: row.item,
                display_name: projected.display_name,
                display_path: projected.display_path,
                logical_size: projected.logical_size,
                allocated_size: projected.allocated_size,
                groups: row
                    .groups
                    .iter()
                    .filter_map(|index| meta.group_ids.get(*index).cloned())
                    .collect(),
                undecidable_reason_keys: row
                    .undecidable_reasons
                    .iter()
                    .map(|key| (*key).to_owned())
                    .collect(),
            });
        }
        Ok(GroupMembers {
            classification_ref: request.classification_ref,
            result_version: request.result_version,
            data_source: Some(classification.version().data_source()),
            start: request.start,
            total: u32::try_from(page.total).map_err(|_| internal())?,
            rows,
            sort: QueryStrategy {
                id: key.id,
                revision: key.revision,
            },
        })
    }
}

pub(super) struct Explain(pub(super) Arc<Services>);

impl UseCase for Explain {
    const NAME: &'static str = "group.explain";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = GroupExplainRequest;
    type Response = GroupExplanation;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<GroupExplanation, EngineError> {
        let services = &self.0;
        let (classification, meta) = services
            .registered
            .get(&request.classification_ref, &services.assembly.store)?;
        same_version(&classification, &request.item.result_version)?;
        let explanations = classification
            .explain(&request.item, &services.registry)
            .map_err(read_error)?;
        let rules = meta
            .group_ids
            .iter()
            .zip(explanations)
            .map(|(group_id, explanation)| GroupRuleDecision {
                group_id: group_id.clone(),
                decision: decision(explanation.decision),
                reason_key: explanation.decision.reason_key().map(str::to_owned),
                nodes: explanation
                    .nodes
                    .into_iter()
                    .map(|node| GroupConditionNode {
                        path: node.path,
                        predicate: node
                            .predicate
                            .map(|(id, revision)| QueryStrategy { id, revision }),
                        decision: decision(node.decision),
                    })
                    .collect(),
            })
            .collect();
        let version = classification.version();
        let projected = search_page::row(version, &request.item).map_err(|_| internal())?;
        Ok(GroupExplanation {
            data_source: Some(version.data_source()),
            observed_at: version
                .item_observed_at(request.item.item_no)
                .or(version.observed_at())
                .cloned(),
            classification_ref: request.classification_ref,
            display_path: projected.display_path,
            item: request.item,
            rules,
        })
    }
}

fn decision(value: Decision) -> GroupDecision {
    match value {
        Decision::Match => GroupDecision::Match,
        Decision::NoMatch => GroupDecision::NoMatch,
        Decision::Unknown | Decision::RuleUnavailable => GroupDecision::Undecidable,
    }
}

pub(super) struct Totals(pub(super) Arc<Services>);

impl UseCase for Totals {
    const NAME: &'static str = "group.totals";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = GroupTotalsRequest;
    type Response = GroupTotals;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<GroupTotals, EngineError> {
        let services = &self.0;
        let (classification, meta) = services
            .registered
            .get(&request.classification_ref, &services.assembly.store)?;
        let selected = request
            .group_ids
            .iter()
            .map(|id| group_index(&meta, id))
            .collect::<Result<Vec<_>, _>>()?;
        let totals = classification
            .totals(&selected, || caller.is_cancelled())
            .map_err(read_error)?;
        let version = classification.version();
        let total = |aggregate: &SpaceAggregate| {
            let published = space::response(version, 0, aggregate);
            GroupTotal {
                sizes: published.sizes,
                completeness: published.completeness,
            }
        };
        let union = total(&totals.union);
        let scope = total(&totals.scope);
        let ratios = union
            .sizes
            .iter()
            .zip(&scope.sizes)
            .map(|(top, bottom)| tables::ratio(top.size.basis.clone(), &top.size, &bottom.size))
            .collect();
        let rule = version.size_rule();
        Ok(GroupTotals {
            classification_ref: request.classification_ref,
            result_version: version.version_id().clone(),
            data_source: Some(version.data_source()),
            groups: meta
                .group_ids
                .iter()
                .zip(&totals.groups)
                .map(|(group_id, aggregate)| GroupTotalRow {
                    group_id: group_id.clone(),
                    total: total(aggregate),
                })
                .collect(),
            overlap: total(&totals.overlap),
            overlap_count: u32::try_from(totals.overlap_count).map_err(|_| internal())?,
            unclassified: total(&totals.unclassified),
            undecidable: total(&totals.undecidable),
            union,
            scope,
            ratios,
            size_rule: QueryStrategy {
                id: rule.id.into(),
                revision: rule.revision,
            },
        })
    }
}
