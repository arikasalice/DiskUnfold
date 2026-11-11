//! Derives F06 half-open histogram intervals from an explicitly selected F03 table.
//!
//! This does not select a display unit system or register a product strategy.
use du_core_contracts::size_units::{UNIT_TABLE_REVISION, UnitTable};

use super::histogram::Bin;

/// Boundaries together with the supplied table revision and multiplicative base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitBins {
    pub table_revision: u32,
    pub base: u16,
    pub bins: Vec<Bin>,
}

/// A selected table revision or its boundaries cannot be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnsupportedRevision,
    InvalidTable,
    Overflow,
}

/// Derives one interval per unit, ending with an open interval.
///
/// The caller selects the F03 table and passes the revision it selected. The
/// first interval starts at zero; successive boundaries are powers of the table
/// base. The final displayed unit covers all larger u64 values. Unknown values
/// are not numeric boundaries and remain the responsibility of the histogram.
pub fn from_unit_table(table: &UnitTable, revision: u32) -> Result<UnitBins, Error> {
    if revision != UNIT_TABLE_REVISION {
        return Err(Error::UnsupportedRevision);
    }
    if table.base() < 2 || table.symbols().is_empty() {
        return Err(Error::InvalidTable);
    }
    let mut bins = Vec::with_capacity(table.symbols().len());
    let mut lower = 0;
    let mut upper = u64::from(table.base());
    for index in 1..table.symbols().len() {
        bins.push(Bin {
            lower,
            upper: Some(upper),
        });
        lower = upper;
        if index + 1 < table.symbols().len() {
            upper = upper
                .checked_mul(u64::from(table.base()))
                .ok_or(Error::Overflow)?;
        }
    }
    bins.push(Bin { lower, upper: None });
    Ok(UnitBins {
        table_revision: revision,
        base: table.base(),
        bins,
    })
}
