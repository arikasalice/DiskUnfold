//! F03 space totals over a materialized selection of observed files.

use std::collections::BTreeSet;

use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis,
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead, SpaceAggregate},
};

use super::{Accumulator, unknown};
use crate::{QueryError, ResultVersion, physical::Physical};

/// Admission failure for an internal selection calculation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Item(QueryError),
    Cancelled,
}

/// Calculates a set total from the same observations and identity limit as the scope total.
/// Both baseline size rules use one contribution per identity for a selected set.
/// Directory rows carry subtree aggregates, so a selection containing such rows lacks
/// independent file membership and must not inherit their totals.
pub(crate) fn sum(
    version: &ResultVersion,
    items: &[ItemRef],
    membership_complete: bool,
    mut cancelled: impl FnMut() -> bool,
) -> Result<SpaceAggregate, Error> {
    let mut seen = BTreeSet::new();
    let mut logical = Accumulator::default();
    let mut physical = Physical::with_limit(version.shared_table_limit);
    let mut complete =
        membership_complete && version.completeness() == ResultCompleteness::Complete;
    for reference in items {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let row = version.detail(reference).map_err(Error::Item)?.item;
        complete &= row.kind == ResultItemKind::File;
        if complete && seen.insert(row.item_no) {
            logical.observe(row.size.as_ref());
            let _ = physical.observe_pinned(version, row.item_no as usize);
        }
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if !complete {
        let state = match version.completeness() {
            ResultCompleteness::Complete => ResultCompleteness::Missing,
            state => state,
        };
        let mut output = Accumulator::default().finish(state);
        output.allocated_completeness = state;
        output.unique_physical_completeness = state;
        output.limitations.to_mut().push("space.data_insufficient");
        return Ok(output);
    }

    let mut output = logical.finish(ResultCompleteness::Complete);
    if logical.missing > 0 {
        output.logical = unknown(SizeBasis::Logical);
    }
    physical.finish_selection(&mut output);
    if [&output.logical, &output.allocated, &output.unique_physical]
        .iter()
        .any(|value| value.amount == SizeAmount::Unknown)
    {
        output.limitations.to_mut().push("space.data_insufficient");
    }
    Ok(output)
}
