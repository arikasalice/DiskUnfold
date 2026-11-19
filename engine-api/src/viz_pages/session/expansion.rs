use super::*;
use crate::viz_pages::production::{self, OtherPresentation, RowPresentation};
use du_analysis_index::visualization::{projection, typed_layout::Dispatcher};
use du_core_contracts::visualization::request::{VizExpansion, VizViewport, VizVisibleWindow};
use std::collections::{HashMap, VecDeque};

pub(crate) struct Policy<'a> {
    pub base_depth: u32,
    pub entries: &'a [VizExpansion],
    pub max_shapes: usize,
    pub frame: projection::FolderFrame,
}

/// The header band of an expanded folder, in CSS px (F06 design README section 27).
const HEADER_PX: u64 = 20;
/// The inset of a folder's children from its left, right and bottom edges, in CSS px.
const INSET_PX: u64 = 2;
/// A folder narrower or lower than this, in CSS px, is drawn as one shape.
const MIN_WIDTH_PX: u64 = 70;
/// The header band and the minimum height of a shape label (20 + 36 CSS px).
const MIN_HEIGHT_PX: u64 = 56;

/// The folder frame of a request in model coordinates, from its viewport in CSS px
/// and the visible window those pixels show. A part of a pixel rounds up.
pub(crate) fn folder_frame(
    viewport: &VizViewport,
    window: &VizVisibleWindow,
) -> projection::FolderFrame {
    let units = |px: u64, span: u32, pixels: u32| {
        u32::try_from((px * u64::from(span)).div_ceil(u64::from(pixels))).unwrap_or(u32::MAX)
    };
    let x = |px| {
        units(
            px,
            window.right.saturating_sub(window.left),
            viewport.width.get(),
        )
    };
    let y = |px| {
        units(
            px,
            window.bottom.saturating_sub(window.top),
            viewport.height.get(),
        )
    };
    projection::FolderFrame {
        header: y(HEADER_PX),
        side: x(INSET_PX),
        bottom: y(INSET_PX),
        min_width: x(MIN_WIDTH_PX),
        min_height: y(MIN_HEIGHT_PX),
    }
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
    other: Option<&OtherPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<render::Expanded, render::Error> {
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
    let mut expanded = render::Expanded::default();
    let mut queue: VecDeque<_> = (0..roots).map(|index| (index, initial)).collect();
    while let Some((index, inherited)) = queue.pop_front() {
        if cancelled() {
            return Err(render::Error::Cancelled);
        }
        if model.shape_count() >= policy.max_shapes {
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
        // The caller records the facts each call returns to describe descendant shapes.
        let mut presentations = HashMap::new();
        for child in folder.children {
            if cancelled() {
                return Err(render::Error::Cancelled);
            }
            let child_evidence = evidence(source, child, cancelled)?;
            presentations.insert(
                child.item_no,
                RowPresentation {
                    item: ItemRef {
                        result_version: source.result_version().clone(),
                        item_no: child.item_no,
                    },
                    legend_class_key: child_evidence.legend_class_key.clone(),
                    display_flags: child_evidence.display_flags.clone(),
                },
            );
        }
        let before = model.descendants.len();
        let groups_before = model.detail.groups.len();
        projection::add_framed_child_partition(
            model,
            source.version(),
            index,
            dispatcher,
            policy.max_shapes,
            policy.frame,
            cancelled,
        )
        .map_err(|error| match error {
            projection::Error::Cancelled => render::Error::Cancelled,
            other => render::Error::Projection(other),
        })?;
        // A folder that drew a shape or a group has a table of its own; one that was
        // refused or whose children all stayed behind its count has none, and the page
        // of such a folder is refused as that of a folder that was not expanded.
        let added = &model.descendants[before..];
        let group = model.detail.groups.get(groups_before);
        if !added.is_empty() || group.is_some() {
            let table = production::folder_table_rows(
                source.version(),
                model,
                &item,
                added,
                group,
                &presentations,
                other,
            )
            .map_err(render::Error::Registration)?;
            let shape = match index.checked_sub(roots) {
                None => &model.shapes[index].meta,
                Some(nested) => &model.descendants[nested].meta,
            };
            expanded.tables.push((shape.shape_ref.clone(), table));
        }
        queue.extend((before..model.descendants.len()).map(|added| (roots + added, limit)));
        for group in &model.detail.groups[groups_before..] {
            let rows =
                production::nested_group_rows(source.version(), model, group, &mut presentations)
                    .map_err(render::Error::Registration)?;
            expanded.groups.push((group.group_ref.clone(), rows));
        }
    }
    projection::finish_child_partitions(model, cancelled).map_err(|error| match error {
        projection::Error::Cancelled => render::Error::Cancelled,
        other => render::Error::Projection(other),
    })?;
    Ok(expanded)
}

#[cfg(test)]
mod tests;
