//! Per-child attribution: a child without a provable area does not block its siblings.
//!
//! The all-or-nothing admission in the parent module refuses the whole parent when any
//! child cannot be attributed. This admission decides per child instead, and only the
//! parent itself can still be refused.
use super::{
    AdmissionError, AdmittedChild, Child, Identities, check, gap_covers_parent, inspect_subtree,
    order, partition_bytes,
};
use crate::{
    ResultVersion,
    visualization::{reference, root_ref, value},
};
use du_core_contracts::{
    ItemRef, SizeBasis, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};

/// An immediate child whose area cannot be established, with its original F03 value.
///
/// The value may still be exact. It is retained for the table, but it is not
/// provably disjoint from its siblings, so it is never added to a byte sum.
#[derive(Clone, Debug)]
pub(crate) struct Unattributed {
    pub(crate) item: ItemRef,
    pub(crate) value: SizeValue,
}

/// Attributed children in the admission's sibling order, the others in pinned child
/// order, and the parent's own partitionable byte value. Everything built from the
/// rows assumes only that this order is deterministic and total, which the shared
/// sibling comparison (bytes, raw name, item number) guarantees.
pub(crate) struct Partial {
    pub(crate) rows: Vec<AdmittedChild>,
    pub(crate) unattributed: Vec<Unattributed>,
    pub(crate) parent_bytes: u64,
}

struct Entry {
    item_no: u32,
    raw: SizeValue,
    child: Option<AdmittedChild>,
}

/// Admits each immediate child on its own evidence.
///
/// A child is attributed when its subtree has exact observations whose sum equals its
/// F03 value. A file shared with another sibling makes every sharing sibling
/// unattributed, regardless of read order. The parent is refused only when its own
/// value cannot partition, a scope gap covers it, the attributed sum exceeds it, the
/// sum differs from it while every child is attributed. Attributed children follow
/// the same sibling order as the all-or-nothing admission. Cancellation is checked as
/// in that admission.
pub(crate) fn admit_partial_cancellable(
    version: &ResultVersion,
    parent: &ItemRef,
    basis: &SizeBasis,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Partial, AdmissionError> {
    check(cancelled)?;
    let parent_no = root_ref(version, parent).map_err(|_| AdmissionError::InvalidParent)?;
    if gap_covers_parent(version, parent_no, cancelled)? {
        return Err(AdmissionError::NoAttribution);
    }
    let allow_lower_bound =
        *basis == SizeBasis::Logical && version.completeness() == ResultCompleteness::Partial;
    let parent_value =
        value(version, parent_no, basis).map_err(|_| AdmissionError::NoAttribution)?;
    let parent_bytes = partition_bytes(&parent_value, basis, allow_lower_bound)
        .ok_or(AdmissionError::NoAttribution)?;
    let mut identities = Identities::default();
    let mut entries = Vec::with_capacity(version.children(parent_no).len());
    for child in version.children(parent_no) {
        check(cancelled)?;
        let raw =
            value(version, child.item_no, basis).map_err(|_| AdmissionError::NoAttribution)?;
        let observed =
            match inspect_subtree(version, child.item_no, basis, &mut identities, cancelled) {
                Ok(bytes) => Some(bytes),
                Err(AdmissionError::NoAttribution) => None,
                Err(other) => return Err(other),
            };
        let child_row = observed
            .filter(|bytes| partition_bytes(&raw, basis, allow_lower_bound) == Some(*bytes))
            .map(|bytes| AdmittedChild {
                child: Child {
                    item: reference(version, child.item_no),
                    value: raw.clone(),
                    area_attributed: true,
                },
                bytes,
            });
        entries.push(Entry {
            item_no: child.item_no,
            raw,
            child: child_row,
        });
    }
    let mut rows = Vec::with_capacity(entries.len());
    let mut unattributed = Vec::new();
    let mut total = 0_u64;
    for entry in entries {
        check(cancelled)?;
        match entry.child {
            Some(row) if !identities.conflicted.contains(&entry.item_no) => {
                total = total
                    .checked_add(row.bytes)
                    .ok_or(AdmissionError::NoAttribution)?;
                rows.push(row);
            }
            _ => unattributed.push(Unattributed {
                item: reference(version, entry.item_no),
                value: entry.raw,
            }),
        }
    }
    if total > parent_bytes || (unattributed.is_empty() && total != parent_bytes) {
        return Err(AdmissionError::NoAttribution);
    }
    let rows = order(version, rows, cancelled)?;
    check(cancelled)?;
    Ok(Partial {
        rows,
        unattributed,
        parent_bytes,
    })
}
