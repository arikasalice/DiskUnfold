//! The reason keys of a model's remainder, and the relation between its totals.

use super::{VizTotal, VizTotals};
use crate::{SizeAmount, SizeBasis, visualization::quantity::VizAmount};

/// The remainder holds siblings that were left out because they are below the minimum
/// shape area or lack coordinate precision.
pub const REMAINDER_TOO_SMALL: &str = "viz.remainder.too_small";

/// The remainder holds space that no single item can carry, such as a file shared between
/// siblings that the scope counts once.
pub const REMAINDER_UNATTRIBUTED: &str = "viz.remainder.unattributed";

impl VizTotal {
    /// Whether the total carries its F03 source size on the given basis with the same value.
    pub(super) fn matches(&self, basis: Option<&SizeBasis>) -> bool {
        match (&self.source_size, basis) {
            (None, None) => true,
            (Some(size), Some(basis)) => {
                &size.basis == basis
                    && self.quantity.unit() == "bytes"
                    && match (&size.amount, self.quantity.amount()) {
                        (SizeAmount::Exact { bytes }, VizAmount::Exact { value })
                        | (SizeAmount::LowerBound { bytes }, VizAmount::LowerBound { value }) => {
                            i128::from(bytes.get()) == value.get()
                        }
                        (SizeAmount::Unknown, VizAmount::Unknown { .. }) => true,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

impl VizTotals {
    /// Checks the cause keys of the remainder and, where the values allow it, that the
    /// totals add up.
    ///
    /// - When the scope, the drawn, grouped and filtered-out totals and a remainder are
    ///   all exact and in one unit, they add up: drawn + grouped + filtered out +
    ///   remainder = scope, and the remainder is not negative.
    /// - Without a remainder, exact totals in one unit may leave a gap but never exceed
    ///   the scope.
    /// - A lower bound or an unknown value among them switches the sum check off, and
    ///   values of different units are never added.
    /// - The cause keys need a remainder, and they are not empty, sorted and without
    ///   duplicates. Keys that are not registered are accepted.
    pub(super) fn validate_breakdown(&self) -> Result<(), &'static str> {
        self.validate_reason_keys()?;
        if self
            .remainder
            .as_ref()
            .and_then(exact)
            .is_some_and(|(_, value)| value < 0)
        {
            return Err("negative remainder");
        }
        self.validate_sum()
    }

    fn validate_reason_keys(&self) -> Result<(), &'static str> {
        let Some(keys) = &self.remainder_reason_keys else {
            return Ok(());
        };
        let ordered = keys
            .windows(2)
            .all(|pair| matches!(pair, [first, second] if first < second));
        if self.remainder.is_none() || keys.iter().any(String::is_empty) || !ordered {
            return Err("invalid remainder reason keys");
        }
        Ok(())
    }

    fn validate_sum(&self) -> Result<(), &'static str> {
        let Some((unit, scope)) = exact(&self.scope) else {
            return Ok(());
        };
        let mut sum = 0_i128;
        for part in [&self.drawn, &self.grouped, &self.filtered_out]
            .into_iter()
            .chain(self.remainder.as_ref())
        {
            match exact(part) {
                Some((part_unit, value)) if part_unit == unit => sum += value,
                _ => return Ok(()),
            }
        }
        match (self.remainder.is_some(), sum == scope, sum <= scope) {
            (true, false, _) => Err("totals do not add up to the scope"),
            (false, _, false) => Err("totals exceed the scope"),
            _ => Ok(()),
        }
    }
}

/// The unit and value of a total that is exact.
fn exact(total: &VizTotal) -> Option<(&str, i128)> {
    match total.quantity.amount() {
        VizAmount::Exact { value } => Some((total.quantity.unit(), value.get())),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
