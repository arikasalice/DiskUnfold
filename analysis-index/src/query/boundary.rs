//! Integer size-boundary normalization for the registered F04 size predicate.
//!
//! This module accepts typed values only. It does not define query-language tokens,
//! decimal rounding, or how text is parsed into a unit and comparison.
use super::{Basis, Boundary, Condition, Predicate};

/// An explicitly selected unit from the SI or IEC size-unit table, version 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Byte,
    Kilobyte,
    Megabyte,
    Gigabyte,
    Terabyte,
    Petabyte,
    Kibibyte,
    Mebibyte,
    Gibibyte,
    Tebibyte,
    Pebibyte,
}

impl Unit {
    fn bytes(self) -> u64 {
        match self {
            Self::Byte => 1,
            Self::Kilobyte => 1_000,
            Self::Megabyte => 1_000_000,
            Self::Gigabyte => 1_000_000_000,
            Self::Terabyte => 1_000_000_000_000,
            Self::Petabyte => 1_000_000_000_000_000,
            Self::Kibibyte => 1 << 10,
            Self::Mebibyte => 1 << 20,
            Self::Gibibyte => 1 << 30,
            Self::Tebibyte => 1 << 40,
            Self::Pebibyte => 1 << 50,
        }
    }
}

/// The four supported size-boundary relations, independent of the unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    AtLeast,
    GreaterThan,
    AtMost,
    LessThan,
}

/// A size boundary cannot be represented as an unsigned 64-bit byte count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalizeError {
    Overflow,
}

/// Converts a whole-unit size threshold into the existing normalized size condition.
///
/// The conversion uses checked integer arithmetic and preserves both the size basis and
/// boundary inclusion. Values requiring fractional units must be resolved by a future
/// published input contract; this function never rounds or saturates them.
pub fn normalize_size(
    basis: Basis,
    comparison: Comparison,
    amount: u64,
    unit: Unit,
) -> Result<Condition, NormalizeError> {
    let bytes = amount
        .checked_mul(unit.bytes())
        .ok_or(NormalizeError::Overflow)?;
    let boundary = match comparison {
        Comparison::AtLeast => Boundary::AtLeast(bytes),
        Comparison::GreaterThan => Boundary::GreaterThan(bytes),
        Comparison::AtMost => Boundary::AtMost(bytes),
        Comparison::LessThan => Boundary::LessThan(bytes),
    };
    Ok(Condition::Predicate(Predicate::Size(basis, boundary)))
}
