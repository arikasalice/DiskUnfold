//! Per-child attribution: a child without a provable area does not block its siblings.
//!
//! The all-or-nothing admission in the parent module refuses the whole parent when any
//! child cannot be attributed. This admission decides per child instead, and only the
//! parent itself can still be refused.
use super::{
    AdmissionError, AdmittedChild, Child, Identities, Observed, check, gap_covers_parent,
    inspect_subtree, lower_bound_partitions, order, partition_bytes,
};
use crate::{
    ResultVersion,
    visualization::{reference, root_ref, value},
};
use du_core_contracts::{ItemRef, SizeAmount, SizeBasis, SizeValue};

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
    let allow_lower_bound = lower_bound_partitions(basis);
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
        let observed = match inspect_subtree(
            version,
            child.item_no,
            basis,
            &mut identities,
            allow_lower_bound,
            cancelled,
        ) {
            Ok(observed) => Some(observed),
            Err(AdmissionError::NoAttribution) => None,
            Err(other) => return Err(other),
        };
        let child_row = observed
            .filter(|observed| is_observed_value(&raw, basis, observed, allow_lower_bound))
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

// The F03 value of a sibling is what its subtree measured. Over a subtree with
// unmeasured parts only a lower bound can be that value.
fn is_observed_value(
    raw: &SizeValue,
    basis: &SizeBasis,
    observed: &Observed,
    allow_lower_bound: bool,
) -> bool {
    let lower = matches!(raw.amount, SizeAmount::LowerBound { .. });
    (lower || !observed.unmeasured)
        && partition_bytes(raw, basis, allow_lower_bound) == Some(observed.bytes)
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
        };
        let measured = Observed {
            bytes: 20,
            unmeasured: false,
        };
        assert!(!is_observed_value(&exact, &basis, &with_gap, true));
        assert!(is_observed_value(&lower, &basis, &with_gap, true));
        assert!(is_observed_value(&exact, &basis, &measured, true));
        // A lower bound over fully measured parts happens when the whole result is one.
        assert!(is_observed_value(&lower, &basis, &measured, true));
        // Whatever the state, the measured bytes must be the value.
        let short = Observed {
            bytes: 19,
            unmeasured: true,
        };
        assert!(!is_observed_value(&lower, &basis, &short, true));
        // Where a lower bound may not take part, it is never the observed value.
        assert!(!is_observed_value(&lower, &basis, &with_gap, false));
    }
}
