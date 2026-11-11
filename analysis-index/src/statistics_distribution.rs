//! Independent F26 distribution calculations over caller-selected pinned rows.
//!
//! Classification and size observations belong to their source producers. This
//! module does not select defaults, derive file types, or define wire keys.
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue};
use std::collections::BTreeMap;

pub mod strategies;

/// An upstream classification result, preserving missing and unclassified values.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Missing,
    NoExtension,
    Unclassified,
    Key(Vec<u8>),
}

/// One selected row with a source-owned classification and size observation.
pub struct Row<'a> {
    pub size: Option<&'a SizeValue>,
    pub category: Category,
}

/// Counts and observed bytes; unknown members never contribute invented bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Amounts {
    pub count: u64,
    pub known_bytes: u128,
    pub unknown_count: u64,
    pub lower_bound_count: u64,
}

/// A category and its observed, potentially incomplete totals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryRow {
    pub category: Category,
    pub amounts: Amounts,
}

/// One interval, followed by independent outside and unknown aggregates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bins {
    pub intervals: Vec<Amounts>,
    pub outside: Amounts,
    pub unknown: Amounts,
}

/// Input boundary failures, arithmetic overflow, or cancellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidBoundaries,
    BasisMismatch,
    Overflow,
    Cancelled,
}

#[derive(Clone, Copy)]
enum Number {
    Known(u64, bool),
    Unknown,
}

/// Assigns rows using caller-provided ordered boundaries and source size basis.
///
/// Intervals include their lower endpoint and exclude the upper endpoint except
/// for the final interval, which includes both. Lower bounds are assigned by
/// their reported bytes and counted separately. At least two strictly increasing
/// boundaries are required; no default boundaries are assumed.
pub fn bins(
    rows: &[Row<'_>],
    basis: &SizeBasis,
    boundaries: &[u64],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Bins, Error> {
    if boundaries.len() < 2 || boundaries.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(Error::InvalidBoundaries);
    }
    let mut result = Bins {
        intervals: vec![Amounts::default(); boundaries.len() - 1],
        outside: Amounts::default(),
        unknown: Amounts::default(),
    };
    for row in rows {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let number = number(row.size, basis)?;
        let destination = match number {
            Number::Unknown => &mut result.unknown,
            Number::Known(bytes, _)
                if bytes < boundaries[0] || bytes > boundaries[boundaries.len() - 1] =>
            {
                &mut result.outside
            }
            Number::Known(bytes, _) => {
                let index = boundaries
                    .partition_point(|bound| *bound <= bytes)
                    .saturating_sub(1)
                    .min(result.intervals.len() - 1);
                &mut result.intervals[index]
            }
        };
        add(destination, number)?;
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(result)
}

/// Groups by upstream category in deterministic internal key order.
///
/// `known_bytes` is only the sum of reported values; unknown and lower-bound
/// member counts must be retained when interpreting the category total. Final
/// size ordering is left to a producer with the complete F03 total contract.
pub fn by_type(
    rows: &[Row<'_>],
    basis: &SizeBasis,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<CategoryRow>, Error> {
    let mut groups = BTreeMap::<Category, Amounts>::new();
    for row in rows {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let value = number(row.size, basis)?;
        add(groups.entry(row.category.clone()).or_default(), value)?;
    }
    let output: Vec<_> = groups
        .into_iter()
        .map(|(category, amounts)| CategoryRow { category, amounts })
        .collect();
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(output)
}

fn number(value: Option<&SizeValue>, basis: &SizeBasis) -> Result<Number, Error> {
    let Some(value) = value else {
        return Ok(Number::Unknown);
    };
    if &value.basis != basis {
        return Err(Error::BasisMismatch);
    }
    if value.evidence == SizeEvidence::Unknown {
        return Ok(Number::Unknown);
    }
    match &value.amount {
        SizeAmount::Exact { bytes } => Ok(bytes
            .as_str()
            .parse()
            .map_or(Number::Unknown, |n| Number::Known(n, false))),
        SizeAmount::LowerBound { bytes } => Ok(bytes
            .as_str()
            .parse()
            .map_or(Number::Unknown, |n| Number::Known(n, true))),
        SizeAmount::Unknown => Ok(Number::Unknown),
    }
}

fn add(total: &mut Amounts, number: Number) -> Result<(), Error> {
    total.count = total.count.checked_add(1).ok_or(Error::Overflow)?;
    match number {
        Number::Unknown => {
            total.unknown_count = total.unknown_count.checked_add(1).ok_or(Error::Overflow)?
        }
        Number::Known(bytes, lower) => {
            total.known_bytes = total
                .known_bytes
                .checked_add(u128::from(bytes))
                .ok_or(Error::Overflow)?;
            if lower {
                total.lower_bound_count = total
                    .lower_bound_count
                    .checked_add(1)
                    .ok_or(Error::Overflow)?;
            }
        }
    }
    Ok(())
}
