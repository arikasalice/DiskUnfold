use super::*;
use du_analysis_index::visualization::{projection, typed_layout::Dispatcher};
use du_core_contracts::visualization::request::VizExpansion;
use std::collections::{HashMap, VecDeque};

pub(crate) struct Policy<'a> {
    pub base_depth: u32,
    pub entries: &'a [VizExpansion],
    pub max_shapes: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExpansionError {
    Limit,
    Invalid,
    Cancelled,
}

/// Validates every entry before removing zero or logically inactive entries.
/// Logical activity uses the pinned topology and open ancestor depths, not geometry.
pub(crate) fn normalize_expansions(
    source: &CurrentAnalysis,
    parent: &ItemRef,
    base_depth: u32,
    entries: &[VizExpansion],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<VizExpansion>, ExpansionError> {
    if cancelled() {
        return Err(ExpansionError::Cancelled);
    }
    if entries.len() > 64 {
        return Err(ExpansionError::Limit);
    }
    let items = source.version().items();
    if parent.result_version != *source.result_version() {
        return Err(ExpansionError::Invalid);
    }
    if items
        .get(parent.item_no as usize)
        .is_none_or(|item| item.kind != du_core_contracts::result::ResultItemKind::Directory)
    {
        return Err(ExpansionError::Invalid);
    }
    let mut seen = HashMap::new();
    let mut candidates = Vec::with_capacity(entries.len());
    for entry in entries {
        if cancelled() {
            return Err(ExpansionError::Cancelled);
        }
        if entry.extra_depth > 8 {
            return Err(ExpansionError::Limit);
        }
        if entry.item_ref.result_version != *source.result_version()
            || seen.insert(entry.item_ref.item_no, ()).is_some()
        {
            return Err(ExpansionError::Invalid);
        }
        let item = items
            .get(entry.item_ref.item_no as usize)
            .ok_or(ExpansionError::Invalid)?;
        if item.kind != du_core_contracts::result::ResultItemKind::Directory {
            return Err(ExpansionError::Invalid);
        }
        let mut current = item.item_no;
        let mut ancestors = Vec::new();
        while current != parent.item_no {
            if cancelled() {
                return Err(ExpansionError::Cancelled);
            }
            if ancestors.len() >= items.len() {
                return Err(ExpansionError::Invalid);
            }
            current = items
                .get(current as usize)
                .and_then(|node| node.parent)
                .ok_or(ExpansionError::Invalid)?;
            ancestors.push(current);
        }
        candidates.push((ancestors.len(), ancestors, entry.clone()));
    }
    candidates.sort_unstable_by_key(|(depth, _, entry)| (*depth, entry.item_ref.item_no));
    let mut active = HashMap::<u32, u32>::new();
    let mut normalized = Vec::new();
    for (depth, ancestors, entry) in candidates {
        if cancelled() {
            return Err(ExpansionError::Cancelled);
        }
        let extra = ancestors
            .iter()
            .filter_map(|item| active.get(item))
            .copied()
            .max()
            .unwrap_or(0);
        if entry.extra_depth > 0 && depth <= base_depth.saturating_add(extra) as usize {
            active.insert(entry.item_ref.item_no, entry.extra_depth);
            normalized.push(entry);
        }
    }
    normalized.sort_unstable_by_key(|entry| entry.item_ref.item_no);
    if cancelled() {
        return Err(ExpansionError::Cancelled);
    }
    Ok(normalized)
}

pub(super) fn extend(
    model: &mut projection::Model,
    source: &CurrentAnalysis,
    dispatcher: &Dispatcher,
    policy: Policy<'_>,
    evidence: &mut impl FnMut(
        &CurrentAnalysis,
        &ResultItem,
        &mut dyn FnMut() -> bool,
    ) -> Result<ChildPolicyEvidence, render::Error>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), render::Error> {
    let entries = normalize_expansions(
        source,
        &model.parent,
        policy.base_depth,
        policy.entries,
        cancelled,
    )
    .map_err(|error| match error {
        ExpansionError::Cancelled => render::Error::Cancelled,
        _ => render::Error::InvalidEvidence,
    })?
    .into_iter()
    .map(|entry| (entry.item_ref.item_no, entry.extra_depth))
    .collect::<HashMap<_, _>>();
    let initial = policy
        .base_depth
        .saturating_add(entries.get(&model.parent.item_no).copied().unwrap_or(0));
    let roots = model.shapes.len();
    let mut queue: VecDeque<_> = (0..roots).map(|index| (index, initial)).collect();
    while let Some((index, inherited)) = queue.pop_front() {
        if cancelled() {
            return Err(render::Error::Cancelled);
        }
        if model.shapes.len().saturating_add(model.descendants.len()) >= policy.max_shapes {
            break;
        }
        let (item, depth) = if index < roots {
            let shape = &model.shapes[index];
            (
                model.rows[shape.row_index].item.clone(),
                shape.meta.visible_depth,
            )
        } else {
            let shape = &model.descendants[index - roots];
            (shape.item.clone(), shape.meta.visible_depth)
        };
        let limit = inherited.max(
            policy
                .base_depth
                .saturating_add(entries.get(&item.item_no).copied().unwrap_or(0)),
        );
        if depth >= limit {
            continue;
        }
        let observed = source
            .version()
            .items()
            .get(item.item_no as usize)
            .ok_or(render::Error::InvalidEvidence)?;
        if observed.kind != du_core_contracts::result::ResultItemKind::Directory {
            continue;
        }
        let folder = du_analysis_index::browse::open(source.version(), Some(&item))
            .map_err(|_| render::Error::InvalidEvidence)?;
        let mut keys = Vec::new();
        for child in folder.children {
            if cancelled() {
                return Err(render::Error::Cancelled);
            }
            if let Some(path_compare_key) = evidence(source, child, cancelled)?.path_compare_key {
                keys.push(du_analysis_index::visualization::treemap::ChildKey {
                    item: ItemRef {
                        result_version: source.result_version().clone(),
                        item_no: child.item_no,
                    },
                    path_compare_key,
                });
            }
        }
        let before = model.descendants.len();
        projection::add_child_partition(
            model,
            source.version(),
            index,
            &keys,
            dispatcher,
            policy.max_shapes,
            cancelled,
        )
        .map_err(|error| match error {
            projection::Error::Cancelled => render::Error::Cancelled,
            other => render::Error::Projection(other),
        })?;
        queue.extend((before..model.descendants.len()).map(|added| (roots + added, limit)));
    }
    projection::finish_child_partitions(model, cancelled).map_err(|error| match error {
        projection::Error::Cancelled => render::Error::Cancelled,
        other => render::Error::Projection(other),
    })
}

#[cfg(test)]
mod tests;
