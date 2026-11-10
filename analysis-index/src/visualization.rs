//! Read-only hierarchy inputs for visualization over a pinned result version.
//!
//! This module supplies rows and visibility facts, not layout coordinates,
//! serialized IPC contracts, cached models, or geometry totals.
pub mod bar;
pub mod bundle;
pub mod geometry;
pub mod group_page;
pub mod histogram;
pub mod live_models;
pub mod model_links;
pub mod ratio;
pub mod registry;
pub mod sankey;
pub mod squarify;
pub mod treemap;
pub mod trend;
pub mod typed_layout;
pub mod waterfall;
use crate::ResultVersion;
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultItem, ResultItemKind, ResultVersionRead},
};
use std::collections::{BTreeMap, HashSet};

/// The published view policy permits at most eight additional depth levels.
pub const MAX_EXTRA_DEPTH: i64 = 8;
/// The published view policy permits at most 64 expansion entries.
pub const MAX_EXPANSIONS: usize = 64;

/// One requested per-directory extension of the global base depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansion {
    pub item: ItemRef,
    pub extra_depth: i64,
}

/// Validation failures for the internal hierarchy input request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HierarchyError {
    /// The scope or navigation reference does not name this pinned version.
    VersionMismatch,
    /// The scope or navigation reference is missing, not a directory, or outside scope.
    InvalidRoot,
    /// An expansion violates the published viewport validation rules.
    ViewportInvalid,
    /// The pinned version does not expose the required aggregate.
    Unavailable,
}

/// Area eligibility; a lower-bound zero is distinct from a confirmed exact zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaValue {
    Positive,
    ExactZero,
    LowerBoundZero,
    Unknown,
}

/// A row inside the requested depth boundary, with no geometry or inferred bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HierarchyRow {
    pub item: ItemRef,
    /// The visible parent row, or `None` for the navigation root.
    pub parent_item_no: Option<u32>,
    pub visible_depth: u32,
    pub raw_name: Vec<u8>,
    pub kind: ResultItemKind,
    /// The original F03 value, retaining its basis, amount state, and evidence.
    pub value: SizeValue,
    pub area_value: AreaValue,
    /// A known positive value cannot be laid out inside a parent without area.
    pub suppressed_by_parent: bool,
    /// Immediate children beyond the depth boundary, not a recursive total.
    pub omitted_children: usize,
}

/// A deterministic read-only input; rows are not shapes and must not be summed
/// across ancestor and descendant levels. Sibling subtree aggregates may also
/// overlap when the same file is linked in multiple branches; a layout must use
/// the F03 attribution contract rather than assume they form a partition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hierarchy {
    pub result_version: Id,
    pub scope_item_no: u32,
    pub navigation_item_no: u32,
    pub completeness: ResultCompleteness,
    pub rows: Vec<HierarchyRow>,
    /// Valid nonzero expansions actually reached within the depth boundary.
    pub active_expansions: Vec<Expansion>,
}

/// Produces hierarchy rows from one pinned version without reading the filesystem.
///
/// Expansion references are validated against the aggregation scope before
/// unreachable entries are removed. Siblings retain their pinned source order;
/// layout and its tie-breaking rules are not selected here. No assumption that
/// unknown size is zero is made.
pub fn hierarchy(
    version: &ResultVersion,
    scope: &ItemRef,
    navigation: &ItemRef,
    basis: SizeBasis,
    base_depth: u32,
    expansions: &[Expansion],
) -> Result<Hierarchy, HierarchyError> {
    let items = version.items();
    let scope_no = root_ref(version, scope)?;
    let root_no = root_ref(version, navigation)?;
    if !descendant(items, root_no, scope_no) {
        return Err(HierarchyError::InvalidRoot);
    }
    let expansion_map = validate_expansions(version, scope_no, expansions)?;
    let mut result = Hierarchy {
        result_version: version.version_id().clone(),
        scope_item_no: scope_no,
        navigation_item_no: root_no,
        completeness: version.completeness(),
        rows: Vec::new(),
        active_expansions: Vec::new(),
    };
    let mut work = vec![Pending {
        item_no: root_no,
        depth: 0,
        limit: base_depth,
        parent_has_area: true,
    }];
    while let Some(pending) = work.pop() {
        let item = items
            .get(pending.item_no as usize)
            .ok_or(HierarchyError::Unavailable)?;
        let value = value(version, pending.item_no, &basis)?;
        let area_value = area(&value);
        let visible_area = pending.parent_has_area && area_value == AreaValue::Positive;
        let mut limit = pending.limit;
        if let Some(extra) = expansion_map
            .get(&pending.item_no)
            .copied()
            .filter(|_| pending.parent_has_area && area_value == AreaValue::Positive)
        {
            limit = limit.max(base_depth.saturating_add(extra));
            result.active_expansions.push(Expansion {
                item: reference(version, pending.item_no),
                extra_depth: i64::from(extra),
            });
        }
        let children = version.children(pending.item_no);
        let omitted_children = if pending.depth >= limit {
            children.len()
        } else {
            0
        };
        result.rows.push(HierarchyRow {
            item: reference(version, pending.item_no),
            parent_item_no: (pending.item_no != root_no)
                .then_some(item.parent)
                .flatten(),
            visible_depth: pending.depth,
            raw_name: item.name.clone(),
            kind: item.kind,
            value,
            area_value,
            suppressed_by_parent: !pending.parent_has_area,
            omitted_children,
        });
        if pending.depth < limit {
            for child in children.iter().rev() {
                work.push(Pending {
                    item_no: child.item_no,
                    depth: pending.depth + 1,
                    limit,
                    parent_has_area: visible_area,
                });
            }
        }
    }
    result
        .active_expansions
        .sort_by_key(|item| item.item.item_no);
    Ok(result)
}

struct Pending {
    item_no: u32,
    depth: u32,
    limit: u32,
    parent_has_area: bool,
}

fn root_ref(version: &ResultVersion, item: &ItemRef) -> Result<u32, HierarchyError> {
    if &item.result_version != version.version_id() {
        return Err(HierarchyError::VersionMismatch);
    }
    match version.items().get(item.item_no as usize) {
        Some(found) if found.kind == ResultItemKind::Directory => Ok(item.item_no),
        _ => Err(HierarchyError::InvalidRoot),
    }
}
fn validate_expansions(
    version: &ResultVersion,
    scope: u32,
    expansions: &[Expansion],
) -> Result<BTreeMap<u32, u32>, HierarchyError> {
    if expansions.len() > MAX_EXPANSIONS {
        return Err(HierarchyError::ViewportInvalid);
    }
    let mut seen = HashSet::new();
    let mut map = BTreeMap::new();
    for expansion in expansions {
        let item = &expansion.item;
        if &item.result_version != version.version_id()
            || !(0..=MAX_EXTRA_DEPTH).contains(&expansion.extra_depth)
            || !seen.insert(item.item_no)
        {
            return Err(HierarchyError::ViewportInvalid);
        }
        if root_ref(version, item).is_err() || !descendant(version.items(), item.item_no, scope) {
            return Err(HierarchyError::ViewportInvalid);
        }
        if expansion.extra_depth > 0 {
            map.insert(item.item_no, expansion.extra_depth as u32);
        }
    }
    Ok(map)
}
fn descendant(items: &[ResultItem], mut item: u32, ancestor: u32) -> bool {
    loop {
        if item == ancestor {
            return true;
        }
        match items.get(item as usize).and_then(|item| item.parent) {
            Some(parent) if parent < item => item = parent,
            _ => return false,
        }
    }
}
fn value(
    version: &ResultVersion,
    item_no: u32,
    basis: &SizeBasis,
) -> Result<SizeValue, HierarchyError> {
    let space = version.space(item_no).ok_or(HierarchyError::Unavailable)?;
    Ok(match basis {
        SizeBasis::Logical => &space.logical,
        SizeBasis::Allocated => &space.allocated,
        SizeBasis::UniquePhysical => &space.unique_physical,
    }
    .clone())
}
fn number(value: &SizeValue) -> Option<u64> {
    if value.evidence == SizeEvidence::Unknown {
        return None;
    }
    match &value.amount {
        SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => {
            bytes.as_str().parse().ok()
        }
        SizeAmount::Unknown => None,
    }
}
fn area(value: &SizeValue) -> AreaValue {
    match (&value.amount, number(value)) {
        (SizeAmount::Exact { .. }, Some(0)) => AreaValue::ExactZero,
        (SizeAmount::LowerBound { .. }, Some(0)) => AreaValue::LowerBoundZero,
        (_, Some(_)) => AreaValue::Positive,
        _ => AreaValue::Unknown,
    }
}
fn reference(version: &ResultVersion, item_no: u32) -> ItemRef {
    ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    }
}
pub mod projection;
