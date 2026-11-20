//! The #242 attribution of a bar chart's Other group from the treemap partition.
use std::collections::HashSet;

use du_core_contracts::{ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

use super::{Error, check};
use crate::{
    ResultVersion,
    visualization::{geometry::Weight, treemap},
};

/// The exact total of `items` when each is a child of `scope` that the treemap
/// attributes on its own evidence (the per-child admission its tail sums), and that
/// total equals `sum`. A member that is not attributed fails closed.
pub(super) fn partition_total(
    version: &ResultVersion,
    scope: &ItemRef,
    basis: &SizeBasis,
    items: &[u32],
    sum: Weight,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<SizeValue, Error> {
    let admitted =
        treemap::admit_partial_cancellable(version, scope, basis, cancelled).map_err(|error| {
            match error {
                treemap::AdmissionError::Cancelled => Error::Cancelled,
                _ => Error::GroupAttributionUnavailable,
            }
        })?;
    let wanted: HashSet<u32> = items.iter().copied().collect();
    let mut bytes = 0_u64;
    let mut evidence = SizeEvidence::Measured;
    let mut found = 0;
    for row in &admitted.rows {
        check(cancelled)?;
        if !wanted.contains(&row.child.item.item_no) {
            continue;
        }
        if !matches!(row.child.value.amount, SizeAmount::Exact { .. }) {
            return Err(Error::GroupAttributionUnavailable);
        }
        found += 1;
        bytes = bytes
            .checked_add(row.bytes)
            .ok_or(Error::GroupAttributionUnavailable)?;
        evidence = weaker(evidence, row.child.value.evidence.clone());
    }
    if found != wanted.len() || Weight::Exact(bytes) != sum {
        return Err(Error::GroupAttributionUnavailable);
    }
    Ok(SizeValue {
        basis: basis.clone(),
        evidence,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    })
}

fn weaker(left: SizeEvidence, right: SizeEvidence) -> SizeEvidence {
    let rank = |evidence: &SizeEvidence| match evidence {
        SizeEvidence::Measured => 0,
        SizeEvidence::OsReported => 1,
        SizeEvidence::Estimated => 2,
        SizeEvidence::Unknown => 3,
    };
    if rank(&right) > rank(&left) {
        right
    } else {
        left
    }
}
