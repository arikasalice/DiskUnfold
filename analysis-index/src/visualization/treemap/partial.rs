//! Per-child attribution: a child without a provable area does not block its siblings.
//!
//! The all-or-nothing admission in the parent module refuses the whole parent when any
//! child cannot be attributed. This admission decides per child instead, and only the
//! parent itself can still be refused.
use super::{
    AdmissionError, AdmittedChild, Child, Identities, Observed, attributed_value, check,
    gap_covers_parent, inspect_subtree, lower_bound_partitions, order, partition_bytes,
};
use crate::{
    ResultVersion,
    visualization::{filter::ItemFilter, reference, root_ref},
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultItemKind,
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
    /// The bytes the rows divide: the parent's own value, less what a filter removed.
    pub(crate) parent_bytes: u64,
    /// Bytes of the files a filter removed from the attributed children; zero without one.
    pub(crate) filtered_bytes: u64,
}

struct Entry {
    item_no: u32,
    raw: SizeValue,
    child: Option<AdmittedChild>,
    /// Kept bytes and unread parts of the subtree, when it could be observed.
    kept: Option<(u64, bool)>,
}

/// Admits each immediate child on its own evidence.
///
/// A child is attributed when its subtree has observations whose sum equals its F03
/// value. The value may be a lower bound (HLD table 22: drawn by its lower bound, the
/// totals marked as such). A link that was not followed is a scope boundary with no bytes
/// of its own, and the value above it is the measured bytes whether F03 calls it exact or
/// a lower bound. An entry that could not be read adds no bytes either and leaves the
/// subtree with a lower bound, attributed by the bytes that were measured; a value
/// claiming to be exact over such a subtree contradicts the observations and is not
/// attributed. A child with no value of its own, and a file shared with another sibling,
/// which makes every sharing sibling unattributed regardless of read order, are counted
/// without an area. The parent is refused only when its own value cannot partition, a
/// scope gap covers it, the attributed sum exceeds it, or the sum differs from it while
/// every child is attributed; lower bounds are compared with lower bounds. Attributed
/// children follow the same sibling order as the all-or-nothing admission. Cancellation
/// is checked as in that admission.
///
/// With a filter, it then keeps only what the filter keeps.
///
/// Attribution is decided on the whole subtree, so a filter never makes a child
/// provable or unprovable. An attributed child takes the kept bytes as its value, a
/// lower bound when its value or its subtree is one; a child that keeps nothing is
/// left out, and what the filter removed from attributed children is counted apart.
/// An unattributed child that may hold a kept file stays without an area, with an
/// unknown value, since its kept share is not provable either.
pub(crate) fn admit_filtered_cancellable(
    version: &ResultVersion,
    parent: &ItemRef,
    basis: &SizeBasis,
    filter: Option<ItemFilter>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Partial, AdmissionError> {
    check(cancelled)?;
    let parent_no = root_ref(version, parent).map_err(|_| AdmissionError::InvalidParent)?;
    if gap_covers_parent(version, parent_no, cancelled)? {
        return Err(AdmissionError::NoAttribution);
    }
    let allow_lower_bound = lower_bound_partitions(basis);
    let parent_value = attributed_value(version, parent_no, basis)?;
    let parent_bytes = partition_bytes(&parent_value, basis, allow_lower_bound)
        .ok_or(AdmissionError::NoAttribution)?;
    let mut identities = Identities::default();
    let mut entries = Vec::with_capacity(version.children(parent_no).len());
    for child in version.children(parent_no) {
        check(cancelled)?;
        let raw = attributed_value(version, child.item_no, basis)?;
        let observed = match inspect_subtree(
            version,
            child.item_no,
            basis,
            &mut identities,
            allow_lower_bound,
            filter,
            cancelled,
        ) {
            Ok(observed) => Some(observed),
            Err(AdmissionError::NoAttribution) => None,
            Err(other) => return Err(other),
        };
        let kept = observed
            .as_ref()
            .map(|observed| (observed.kept, observed.unmeasured));
        let child_row = observed
            .filter(|observed| {
                is_observed_value(child.kind, &raw, basis, observed, allow_lower_bound)
            })
            .map(|observed| AdmittedChild {
                child: Child {
                    item: reference(version, child.item_no),
                    value: raw.clone(),
                    area_attributed: true,
                },
                bytes: observed.bytes,
            });
        entries.push(Entry {
            item_no: child.item_no,
            raw,
            child: child_row,
            kept,
        });
    }
    let mut rows = Vec::with_capacity(entries.len());
    let mut unattributed = Vec::new();
    let mut total = 0_u64;
    let mut filtered_bytes = 0_u64;
    for entry in entries {
        check(cancelled)?;
        match entry.child {
            Some(mut row) if !identities.conflicted.contains(&entry.item_no) => {
                total = total
                    .checked_add(row.bytes)
                    .ok_or(AdmissionError::NoAttribution)?;
                if filter.is_some() {
                    let (kept, unmeasured) = entry.kept.ok_or(AdmissionError::NoAttribution)?;
                    let removed = row
                        .bytes
                        .checked_sub(kept)
                        .ok_or(AdmissionError::NoAttribution)?;
                    filtered_bytes = filtered_bytes
                        .checked_add(removed)
                        .ok_or(AdmissionError::NoAttribution)?;
                    if kept == 0 {
                        continue;
                    }
                    row.child.value = kept_value(&row.child.value, kept, unmeasured);
                    row.bytes = kept;
                }
                rows.push(row);
            }
            _ if filter.is_some() && entry.kept.is_some_and(|(kept, _)| kept == 0) => {}
            _ => unattributed.push(Unattributed {
                item: reference(version, entry.item_no),
                value: if filter.is_some() {
                    unknown(entry.raw.basis)
                } else {
                    entry.raw
                },
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
        parent_bytes: parent_bytes
            .checked_sub(filtered_bytes)
            .ok_or(AdmissionError::NoAttribution)?,
        filtered_bytes,
    })
}

/// The kept bytes of a child as a value in its basis, a lower bound when the child's
/// value is one or its subtree has unread parts.
fn kept_value(value: &SizeValue, kept: u64, unmeasured: bool) -> SizeValue {
    let amount = if unmeasured || matches!(value.amount, SizeAmount::LowerBound { .. }) {
        SizeAmount::LowerBound { bytes: kept.into() }
    } else {
        SizeAmount::Exact { bytes: kept.into() }
    };
    SizeValue {
        basis: value.basis.clone(),
        evidence: value.evidence.clone(),
        amount,
    }
}

fn unknown(basis: SizeBasis) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::Unknown,
        amount: SizeAmount::Unknown,
    }
}

// The F03 value of a sibling is what its subtree measured. Over a subtree with
// unmeasured parts only a lower bound can be that value.
fn is_observed_value(
    kind: ResultItemKind,
    raw: &SizeValue,
    basis: &SizeBasis,
    observed: &Observed,
    allow_lower_bound: bool,
) -> bool {
    let lower = matches!(raw.amount, SizeAmount::LowerBound { .. });
    if lower || !observed.unmeasured {
        let Some(bytes) = partition_bytes(raw, basis, allow_lower_bound) else {
            return false;
        };
        if *basis == SizeBasis::Allocated && kind == ResultItemKind::Directory {
            observed.bytes <= bytes
        } else {
            observed.bytes == bytes
        }
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::SizeEvidence;

    fn value(amount: SizeAmount) -> SizeValue {
        SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount,
        }
    }

    // No builder can make an exact aggregate over a link, so the contradiction is
    // checked on the decision itself.
    #[test]
    fn a_value_claiming_to_be_exact_over_unmeasured_parts_is_not_the_observed_value() {
        let exact = value(SizeAmount::Exact {
            bytes: 20_u64.into(),
        });
        let lower = value(SizeAmount::LowerBound {
            bytes: 20_u64.into(),
        });
        let basis = SizeBasis::Logical;
        let with_gap = Observed {
            bytes: 20,
            unmeasured: true,
            kept: 20,
        };
        let measured = Observed {
            bytes: 20,
            unmeasured: false,
            kept: 20,
        };
        assert!(!is_observed_value(
            ResultItemKind::File,
            &exact,
            &basis,
            &with_gap,
            true
        ));
        assert!(is_observed_value(
            ResultItemKind::File,
            &lower,
            &basis,
            &with_gap,
            true
        ));
        assert!(is_observed_value(
            ResultItemKind::File,
            &exact,
            &basis,
            &measured,
            true
        ));
        // A lower bound over fully measured parts happens when the whole result is one.
        assert!(is_observed_value(
            ResultItemKind::File,
            &lower,
            &basis,
            &measured,
            true
        ));
        // Whatever the state, the measured bytes must be the value.
        let short = Observed {
            bytes: 19,
            unmeasured: true,
            kept: 19,
        };
        assert!(!is_observed_value(
            ResultItemKind::File,
            &lower,
            &basis,
            &short,
            true
        ));
        // Where a lower bound may not take part, it is never the observed value.
        assert!(!is_observed_value(
            ResultItemKind::File,
            &lower,
            &basis,
            &with_gap,
            false
        ));
    }
}
