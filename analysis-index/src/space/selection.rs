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
    cancelled: impl FnMut() -> bool,
) -> Result<SpaceAggregate, Error> {
    let rows = items
        .iter()
        .map(|reference| version.detail(reference).map(|detail| detail.item));
    calculate(version, rows, membership_complete, None, cancelled)
}

/// Calculates a set total from item numbers of `version`, without building references.
///
/// Same rules as [`sum`]; an item number outside the version is `Item(NotFound)`.
pub(crate) fn sum_item_numbers(
    version: &ResultVersion,
    numbers: &[u32],
    membership_complete: bool,
    cancelled: impl FnMut() -> bool,
) -> Result<SpaceAggregate, Error> {
    let rows = numbers.iter().map(|&number| {
        version
            .items()
            .get(number as usize)
            .ok_or(QueryError::NotFound)
    });
    calculate(version, rows, membership_complete, None, cancelled)
}

pub(crate) fn sum_with_range(
    version: &ResultVersion,
    items: &[ItemRef],
    membership_complete: bool,
    cancelled: impl FnMut() -> bool,
) -> Result<(SpaceAggregate, crate::observation_range::RangeIndices), Error> {
    let complete = membership_complete && version.completeness() == ResultCompleteness::Complete;
    let mut range = crate::observation_range::RangeIndices::selection(complete);
    let rows = items
        .iter()
        .map(|reference| version.detail(reference).map(|detail| detail.item));
    let total = calculate(
        version,
        rows,
        membership_complete,
        Some(&mut range),
        cancelled,
    )?;
    Ok((total, range))
}

fn calculate<'a>(
    version: &'a ResultVersion,
    rows: impl Iterator<Item = Result<&'a du_core_contracts::result::ResultItem, QueryError>>,
    membership_complete: bool,
    mut range: Option<&mut crate::observation_range::RangeIndices>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<SpaceAggregate, Error> {
    let mut seen = BTreeSet::new();
    let mut logical = Accumulator::default();
    let mut physical = Physical::with_limit(version.shared_table_limit);
    let mut complete =
        membership_complete && version.completeness() == ResultCompleteness::Complete;
    for row in rows {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let row = row.map_err(Error::Item)?;
        // A link with a known own size is summed like a file; its target is only counted.
        let link = row.kind == ResultItemKind::UntraversedLink
            && row.size.as_ref().is_some_and(super::known_logical);
        let file = row.kind == ResultItemKind::File || link;
        complete &= file;
        if !file && let Some(range) = &mut range {
            range.mark_incomplete();
        }
        if file && (complete || range.is_some()) && seen.insert(row.item_no) {
            if let Some(range) = &mut range {
                range.observe_file(row.item_no, version.item_observed_times.as_deref());
            }
            if !complete {
                continue;
            }
            if link {
                logical.observe_link(row.size.as_ref());
            } else {
                logical.observe(row.size.as_ref());
            }
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
