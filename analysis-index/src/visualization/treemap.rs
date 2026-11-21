//! Admission of one treemap partition from immutable F03 observations.
//!
//! The current index computes physical totals per subtree. It does not retain a
//! selectable attribution rule; this boundary only admits cases whose disjoint
//! attribution is independently established by the retained observations.
//!
//! Siblings are ordered for display by attributed bytes, largest first, and
//! then by the raw name bytes preserved by the index (see `before`). That order
//! is a display order only: it never decides whether two items are the same
//! item and never maps items between result versions.
use super::{filter::ItemFilter, geometry::valid_bounds, reference, root_ref, squarify, value};
use crate::{FileIdentity, ResultVersion};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

mod physical;
use physical::{link_bytes, physical_file};
mod partial;
pub(crate) use partial::{Partial, Unattributed, admit_filtered_cancellable};

/// Original F03 value and item associated with an index in the resulting layout.
#[derive(Clone, Debug)]
pub struct Child {
    pub item: ItemRef,
    pub value: SizeValue,
    /// False when this child is retained without a provable area.
    pub area_attributed: bool,
}

#[derive(Clone, Debug)]
pub struct Partition {
    pub children: Vec<Child>,
    pub layout: squarify::Layout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    InvalidParent,
    /// The operation was cancelled before a complete result was produced.
    Cancelled,
    /// `viz.unavailable.no_attribution`: the source does not establish a partition.
    NoAttribution,
    Layout(squarify::LayoutError),
}

/// One verified row and its exact attributed bytes.
#[derive(Clone, Debug)]
pub(crate) struct AdmittedChild {
    pub(crate) child: Child,
    pub(crate) bytes: u64,
}

/// A disjoint, ordered partition without geometry.
pub struct Admitted {
    pub(crate) rows: Vec<AdmittedChild>,
}

/// Validates and orders one pinned partition without performing layout.
/// Rows of equal bytes follow their raw name bytes, so no caller-supplied
/// ordering evidence is needed.
/// Cancellation is checked while walking gaps and descendant observations, and
/// sorting rows. One name comparison and allocation are not interruptible. No
/// partial result is returned.
pub fn admit_cancellable(
    version: &ResultVersion,
    parent: &ItemRef,
    basis: SizeBasis,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Admitted, AdmissionError> {
    check(cancelled)?;
    let parent_no = root_ref(version, parent).map_err(|_| AdmissionError::InvalidParent)?;
    let children = version.children(parent_no);
    if gap_covers_parent(version, parent_no, cancelled)? {
        return Err(AdmissionError::NoAttribution);
    }
    let mut identities = Identities::default();
    let mut rows = Vec::with_capacity(children.len());
    let mut total = 0_u64;
    // Logical bytes are disjoint by observed path even while the scan is open.
    let allow_lower_bound =
        basis == SizeBasis::Logical && version.completeness() == ResultCompleteness::Partial;
    for child in children {
        check(cancelled)?;
        // A file outside the boundary is an unread range: not drawn and not in the total.
        if version.is_boundary_item(child.item_no) {
            continue;
        }
        let observed = inspect_subtree(
            version,
            child.item_no,
            &basis,
            &mut identities,
            false,
            None,
            cancelled,
        )?
        .bytes;
        let raw =
            value(version, child.item_no, &basis).map_err(|_| AdmissionError::NoAttribution)?;
        if partition_bytes(&raw, &basis, allow_lower_bound) != Some(observed) {
            return Err(AdmissionError::NoAttribution);
        }
        total = total
            .checked_add(observed)
            .ok_or(AdmissionError::NoAttribution)?;
        rows.push(AdmittedChild {
            child: Child {
                item: reference(version, child.item_no),
                value: raw,
                area_attributed: true,
            },
            bytes: observed,
        });
    }
    let parent_value =
        value(version, parent_no, &basis).map_err(|_| AdmissionError::NoAttribution)?;
    if partition_bytes(&parent_value, &basis, allow_lower_bound) != Some(total) {
        return Err(AdmissionError::NoAttribution);
    }
    let rows = order(version, rows, cancelled)?;
    check(cancelled)?;
    Ok(Admitted { rows })
}

/// Validates attribution and calculates the existing one-parent layout.
pub fn layout_parent(
    version: &ResultVersion,
    parent: &ItemRef,
    basis: SizeBasis,
    bounds: squarify::Rect,
) -> Result<Partition, AdmissionError> {
    root_ref(version, parent).map_err(|_| AdmissionError::InvalidParent)?;
    if !valid_bounds(bounds) {
        return Err(AdmissionError::Layout(squarify::LayoutError::InvalidBounds));
    }
    let admitted = admit_cancellable(version, parent, basis, &mut || false)?;
    let weights: Vec<_> = admitted
        .rows
        .iter()
        .map(|row| layout_weight(&row.child.value, row.bytes))
        .collect();
    let layout = squarify::layout(bounds, &weights).map_err(AdmissionError::Layout)?;
    Ok(Partition {
        children: admitted.rows.into_iter().map(|row| row.child).collect(),
        layout,
    })
}

fn check(cancelled: &mut dyn FnMut() -> bool) -> Result<(), AdmissionError> {
    if cancelled() {
        Err(AdmissionError::Cancelled)
    } else {
        Ok(())
    }
}

// Stable merge of lightweight row indexes gives a cancellation check per output.
fn order(
    version: &ResultVersion,
    rows: Vec<AdmittedChild>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<AdmittedChild>, AdmissionError> {
    let mut indices: Vec<usize> = (0..rows.len()).collect();
    let mut scratch = indices.clone();
    let mut width = 1usize;
    while width < indices.len() {
        let mut start = 0usize;
        while start < indices.len() {
            let mid = start.saturating_add(width).min(indices.len());
            let end = mid.saturating_add(width).min(indices.len());
            let (mut left, mut right) = (start, mid);
            for dest in &mut scratch[start..end] {
                check(cancelled)?;
                let take_left = right == end
                    || (left < mid && before(version, &rows[indices[left]], &rows[indices[right]]));
                if take_left {
                    *dest = indices[left];
                    left += 1;
                } else {
                    *dest = indices[right];
                    right += 1;
                }
            }
            start = end;
        }
        std::mem::swap(&mut indices, &mut scratch);
        width = width.saturating_mul(2);
    }
    let mut slots: Vec<_> = rows.into_iter().map(Some).collect();
    let mut output = Vec::with_capacity(indices.len());
    for index in indices {
        check(cancelled)?;
        output.push(slots[index].take().ok_or(AdmissionError::NoAttribution)?);
    }
    Ok(output)
}

/// Display order of two admitted siblings: attributed bytes descending, then
/// the raw name bytes preserved by the index, compared bytewise ascending
/// (a proper prefix sorts first). Scope roots have no parent and sort by their
/// scope-root number instead; the index numbers roots first, in root order, so
/// that number equals their item number. The item number is the last
/// tie-break only so that the order is total; names are unique among siblings.
///
/// The order is for display only. It does not decide whether two rows are the
/// same item and does not map items between result versions. It uses no
/// display name, locale or case folding.
///
/// Because the comparison is on raw bytes, a Windows name (UTF-16LE bytes) is
/// not in human alphabetical order: U+0100 (bytes 00 01) sorts before "a"
/// (bytes 61 00). The rule exists only to make the order deterministic.
///
/// Two siblings with equal bytes compare by their own names alone. While their
/// sizes stay equal, a later version of the same build may insert new siblings
/// between them without changing their relative order.
fn before(version: &ResultVersion, a: &AdmittedChild, b: &AdmittedChild) -> bool {
    let (left, right) = (a.child.item.item_no, b.child.item.item_no);
    b.bytes
        .cmp(&a.bytes)
        .then_with(|| sibling_order(version, left, right))
        .then_with(|| left.cmp(&right))
        .is_le()
}

fn sibling_order(version: &ResultVersion, left: u32, right: u32) -> Ordering {
    match (
        version.items().get(left as usize),
        version.items().get(right as usize),
    ) {
        (Some(a), Some(b)) if a.parent.is_none() && b.parent.is_none() => version
            .root_indices
            .get(left as usize)
            .cmp(&version.root_indices.get(right as usize)),
        (Some(a), Some(b)) => a.name.cmp(&b.name),
        _ => Ordering::Equal,
    }
}

// A gap at an ancestor is not reflected in a descendant aggregate. In that
// case the retained source cannot establish that the selected subtree was read.
fn gap_covers_parent(
    version: &ResultVersion,
    parent_no: u32,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<bool, AdmissionError> {
    let root = *version
        .root_indices
        .get(parent_no as usize)
        .ok_or(AdmissionError::NoAttribution)?;
    let mut path = Vec::new();
    let mut current = parent_no;
    loop {
        check(cancelled)?;
        let item = version
            .items()
            .get(current as usize)
            .ok_or(AdmissionError::NoAttribution)?;
        match item.parent {
            Some(parent) => {
                path.push(&item.name);
                current = parent;
            }
            None => break,
        }
    }
    path.reverse();
    for gap in version.scope_gaps() {
        check(cancelled)?;
        if gap.root_index != root || gap.path.len() > path.len() {
            continue;
        }
        let mut matches = true;
        for (part, actual) in gap.path.iter().zip(&path) {
            check(cancelled)?;
            if part != *actual {
                matches = false;
                break;
            }
        }
        if matches {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Lays out a previously admitted, non-root branch, or retains all of its
/// children without geometry when this branch cannot be partitioned.
///
/// The caller must use `layout_parent` for the navigation root and call this
/// only for a branch whose enclosing rectangle was already admitted. Suppressed
/// children keep their original F03 values; no remainder is redistributed.
pub fn layout_nested_parent(
    version: &ResultVersion,
    parent: &ItemRef,
    basis: SizeBasis,
    bounds: squarify::Rect,
) -> Result<Partition, AdmissionError> {
    match layout_parent(version, parent, basis.clone(), bounds) {
        Ok(partition) => Ok(partition),
        Err(AdmissionError::NoAttribution) => {
            let parent_no = root_ref(version, parent).map_err(|_| AdmissionError::InvalidParent)?;
            let children = version
                .children(parent_no)
                .iter()
                .map(|child| {
                    Ok(Child {
                        item: reference(version, child.item_no),
                        value: value(version, child.item_no, &basis)
                            .map_err(|_| AdmissionError::NoAttribution)?,
                        area_attributed: false,
                    })
                })
                .collect::<Result<Vec<_>, AdmissionError>>()?;
            Ok(Partition {
                children,
                layout: squarify::Layout {
                    shapes: Vec::new(),
                    skipped: Vec::new(),
                },
            })
        }
        Err(error) => Err(error),
    }
}

// Identity -> sibling owner, allocation, link count, observed paths.
type Owners = BTreeMap<FileIdentity, (u32, u64, u64, u64)>;

// Identity ownership, plus every sibling that an identity conflict made unprovable.
#[derive(Default)]
struct Identities {
    owners: Owners,
    conflicted: BTreeSet<u32>,
}
/// What the observations under one sibling add up to on a size basis.
struct Observed {
    /// Bytes of the observations that carry a measured value.
    bytes: u64,
    /// True when the subtree holds an entry that could not be read. It has no value, so
    /// it adds no bytes and the F03 value of the subtree can only be a lower bound. A link
    /// that was not followed is such an entry only when its own size is unknown.
    unmeasured: bool,
    /// Bytes of the observations the filter keeps; all of [bytes](Self::bytes) without one.
    kept: u64,
}

/// Whether a lower-bound F03 value may take part in a partition on this basis.
///
/// A lower bound is a value state, not a reason to withhold area: the partition draws
/// the sibling by its lower bound and the shape, the group and the totals carry the
/// state. Unique physical size has no admitted value yet and keeps its exact-only rule.
fn lower_bound_partitions(basis: &SizeBasis) -> bool {
    matches!(basis, SizeBasis::Logical | SizeBasis::Allocated)
}

/// Sums the measured bytes under a sibling.
///
/// An untraversed link counts its own observed size like a file (F03 FR5.HD1); its target
/// was not followed and is only a counted scope boundary, which lowers neither the value
/// nor the completeness. A link whose own size is unknown is an unread entry. With
/// allow_unmeasured, an unread entry adds no bytes and is reported in the result, so the
/// subtree is placed by its lower bound; without it, such an entry refuses the sibling.
/// With a filter, the bytes it keeps are summed apart; every entry is still checked.
fn inspect_subtree(
    version: &ResultVersion,
    sibling: u32,
    basis: &SizeBasis,
    identities: &mut Identities,
    allow_unmeasured: bool,
    filter: Option<ItemFilter>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Observed, AdmissionError> {
    let mut work = vec![sibling];
    let mut sum = 0_u64;
    let mut kept = 0_u64;
    let mut unmeasured = false;
    let keeps = |no: u32| filter.is_none_or(|filter| filter.keeps(version, no));
    while let Some(no) = work.pop() {
        check(cancelled)?;
        let item = version
            .items()
            .get(no as usize)
            .ok_or(AdmissionError::NoAttribution)?;
        match item.kind {
            ResultItemKind::Directory => {
                for child in version.children(no) {
                    check(cancelled)?;
                    work.push(child.item_no);
                }
            }
            ResultItemKind::File if version.is_boundary_item(no) => {}
            ResultItemKind::File => {
                let bytes = if *basis == SizeBasis::Logical {
                    item.size
                        .as_ref()
                        .and_then(|v| exact(v, basis))
                        .ok_or(AdmissionError::NoAttribution)?
                } else {
                    physical_file(version, no, sibling, basis, identities)?
                };
                sum = sum
                    .checked_add(bytes)
                    .ok_or(AdmissionError::NoAttribution)?;
                if keeps(no) {
                    kept = kept
                        .checked_add(bytes)
                        .ok_or(AdmissionError::NoAttribution)?;
                }
            }
            ResultItemKind::UntraversedLink => {
                match link_bytes(version, no, sibling, basis, identities)? {
                    Some(bytes) => {
                        sum = sum
                            .checked_add(bytes)
                            .ok_or(AdmissionError::NoAttribution)?;
                        if keeps(no) {
                            kept = kept
                                .checked_add(bytes)
                                .ok_or(AdmissionError::NoAttribution)?;
                        }
                    }
                    None if allow_unmeasured => unmeasured = true,
                    None => return Err(AdmissionError::NoAttribution),
                }
            }
            ResultItemKind::Unknown if allow_unmeasured => {
                unmeasured = true;
            }
            _ => return Err(AdmissionError::NoAttribution),
        }
    }
    Ok(Observed {
        bytes: sum,
        unmeasured,
        kept,
    })
}

fn exact(value: &SizeValue, basis: &SizeBasis) -> Option<u64> {
    partition_bytes(value, basis, false)
}

fn partition_bytes(value: &SizeValue, basis: &SizeBasis, allow_lower_bound: bool) -> Option<u64> {
    if value.basis != *basis || value.evidence == SizeEvidence::Unknown {
        return None;
    }
    match &value.amount {
        SizeAmount::Exact { bytes } => Some(bytes.get()),
        SizeAmount::LowerBound { bytes } if allow_lower_bound => Some(bytes.get()),
        _ => None,
    }
}

pub(super) fn layout_weight(value: &SizeValue, bytes: u64) -> squarify::Weight {
    if matches!(value.amount, SizeAmount::LowerBound { .. }) {
        squarify::Weight::LowerBound(bytes)
    } else {
        squarify::Weight::Exact(bytes)
    }
}

#[cfg(test)]
mod order_tests;
