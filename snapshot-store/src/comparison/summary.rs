//! F08 common-range totals from the same set aggregation rule used by F03.
use super::{RootComparison, range, tree};
use crate::storage_match::Correspondence;
use du_core_contracts::{
    SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, SpaceAggregate},
};

/// Side of the pinned comparison whose common-range set is aggregated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Earlier,
    Later,
}

/// Supplies F03 set totals for the same established common range on each side.
///
/// Implementations must use pinned inputs and aggregate the union as a set,
/// including identity deduplication and exclusions. Summing comparison rows or
/// subtree totals does not satisfy this contract. The provider owns membership
/// and must report missing evidence as an incomplete or unknown aggregate.
pub trait SetTotals {
    type Error;
    /// Computes one side's total and checks cancellation while collecting it.
    fn total(
        &mut self,
        side: Side,
        basis: &SizeBasis,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<SpaceAggregate, Self::Error>;
}

/// Both original aggregates and an exact signed common-range delta, if known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Totals {
    pub earlier: SpaceAggregate,
    pub later: SpaceAggregate,
    pub delta: Option<i128>,
}

/// Failure leaves no comparison result or partial totals to publish.
#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    Cancelled,
    InvalidTree,
    Source(E),
}

/// Assembled rows and their separately computed common-range set totals.
pub struct Comparison<'a> {
    pub rows: Vec<tree::Row<'a>>,
    /// Absent when the roots do not establish a common range.
    pub totals: Option<Totals>,
}

/// The pinned data required by the tree comparison and range correspondence.
pub struct Inputs<'a, 'data> {
    pub root: &'a RootComparison,
    pub earlier: &'a tree::Tree<'data>,
    pub later: &'a tree::Tree<'data>,
    pub basis: &'a SizeBasis,
    pub ranges: &'a range::Ranges,
}

/// Assembles the tree and asks the provider for the common set on both sides.
///
/// Incompatible roots preserve their comparison rows without asking for totals.
/// Only exact, complete aggregate amounts produce a delta. The original
/// aggregates remain available even when either amount is uncertain. Decimal
/// byte sizes are parsed as `u64` and widened before subtraction.
///
/// # Errors
/// Returns cancellation, an invalid input tree, or the provider's original error.
pub fn compare<'data, P: SetTotals>(
    inputs: Inputs<'_, 'data>,
    provider: &mut P,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Comparison<'data>, Error<P::Error>> {
    let rows = range::compare(
        inputs.root,
        inputs.earlier,
        inputs.later,
        inputs.basis,
        inputs.ranges,
        &mut cancelled,
    )
    .map_err(|error| match error {
        tree::Error::Cancelled => Error::Cancelled,
        tree::Error::InvalidTree => Error::InvalidTree,
    })?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if inputs.root.correspondence() != Correspondence::Same {
        return Ok(Comparison { rows, totals: None });
    }
    let earlier = provider.total(Side::Earlier, inputs.basis, &mut cancelled);
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let earlier = earlier.map_err(Error::Source)?;
    let later = provider.total(Side::Later, inputs.basis, &mut cancelled);
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let later = later.map_err(Error::Source)?;
    let delta = exact(&earlier, inputs.basis)
        .zip(exact(&later, inputs.basis))
        .map(|(a, b)| i128::from(b) - i128::from(a));
    Ok(Comparison {
        rows,
        totals: Some(Totals {
            earlier,
            later,
            delta,
        }),
    })
}

fn exact(aggregate: &SpaceAggregate, basis: &SizeBasis) -> Option<u64> {
    let (value, completeness): (&SizeValue, _) = match basis {
        SizeBasis::Logical => (&aggregate.logical, aggregate.completeness),
        SizeBasis::Allocated => (&aggregate.allocated, aggregate.allocated_completeness),
        SizeBasis::UniquePhysical => (
            &aggregate.unique_physical,
            aggregate.unique_physical_completeness,
        ),
    };
    if completeness != ResultCompleteness::Complete
        || value.basis != *basis
        || value.evidence == SizeEvidence::Unknown
    {
        return None;
    }
    match &value.amount {
        SizeAmount::Exact { bytes } => bytes.as_str().parse().ok(),
        SizeAmount::LowerBound { .. } | SizeAmount::Unknown => None,
    }
}

#[cfg(test)]
mod tests;
