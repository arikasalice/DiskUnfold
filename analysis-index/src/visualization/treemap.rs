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
use super::{geometry::valid_bounds, reference, root_ref, squarify, value};
use crate::{FileIdentity, ResultVersion, SharingEvidence};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
};
use std::{cmp::Ordering, collections::BTreeMap};

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
    let mut identities = BTreeMap::new();
    let mut rows = Vec::with_capacity(children.len());
    let mut total = 0_u64;
    // Logical bytes are disjoint by observed path even while the scan is open.
    let allow_lower_bound =
        basis == SizeBasis::Logical && version.completeness() == ResultCompleteness::Partial;
    for child in children {
        check(cancelled)?;
        let observed = inspect_subtree(version, child.item_no, &basis, &mut identities, cancelled)?;
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

// Identity, sibling owner, allocation, link count, observed paths.
type Identities = BTreeMap<FileIdentity, (u32, u64, u64, u64)>;
fn inspect_subtree(
    version: &ResultVersion,
    sibling: u32,
    basis: &SizeBasis,
    identities: &mut Identities,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<u64, AdmissionError> {
    let mut work = vec![sibling];
    let mut sum = 0_u64;
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
            }
            _ => return Err(AdmissionError::NoAttribution),
        }
    }
    Ok(sum)
}

/// Attributes one file to its sibling on a physical size basis and returns the bytes it
/// adds to that sibling.
///
/// The file must carry the OS-reported allocation, a file identity and a link count.
/// A file reachable from two siblings, or whose links are not all seen inside one
/// sibling, is refused for both siblings, so no byte is used twice.
///
/// Extent-sharing evidence ([`SharingEvidence`]) matters only when the basis is
/// `UniquePhysical`. F03 sums allocated sizes exactly whatever the sharing evidence is
/// and makes only the unique-physical value unknown (`physical/finish.rs`). The treemap
/// follows the same rule: an allocated partition needs identity and link evidence, not
/// extent-sharing evidence. A scan never establishes that extents are unshared, so
/// demanding it would leave every real allocated partition unattributed.
fn physical_file(
    version: &ResultVersion,
    no: u32,
    sibling: u32,
    basis: &SizeBasis,
    identities: &mut Identities,
) -> Result<u64, AdmissionError> {
    let observation = version
        .physical
        .as_ref()
        .and_then(|v| v.get(no as usize))
        .and_then(Option::as_ref)
        .ok_or(AdmissionError::NoAttribution)?;
    let identity = observation
        .identity
        .as_ref()
        .filter(|id| !id.volume.is_empty() && !id.file.is_empty())
        .ok_or(AdmissionError::NoAttribution)?;
    let count = observation
        .link_count
        .filter(|n| *n > 0)
        .ok_or(AdmissionError::NoAttribution)?;
    if *basis == SizeBasis::UniquePhysical && observation.sharing != SharingEvidence::None {
        return Err(AdmissionError::NoAttribution);
    }
    let size = observation
        .allocated
        .as_ref()
        .filter(|v| v.evidence == SizeEvidence::OsReported)
        .and_then(|v| exact(v, &SizeBasis::Allocated))
        .ok_or(AdmissionError::NoAttribution)?;
    if let Some((owner, previous, links, seen)) = identities.get_mut(identity) {
        if *owner != sibling || *previous != size || *links != count || *seen >= count {
            return Err(AdmissionError::NoAttribution);
        }
        *seen += 1;
        Ok(0)
    } else {
        identities.insert(identity.clone(), (sibling, size, count, 1));
        Ok(size)
    }
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
