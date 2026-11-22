//! The reason keys of a model's remainder, and the relation between its totals.

use super::VizTotal;
use crate::visualization::optional_present;
use crate::{SizeAmount, SizeBasis, visualization::quantity::VizAmount};
use serde::{Deserialize, Serialize};

/// The totals of one model. They have one definition for every format.
///
/// - `scope` is the original aggregate of the pinned scope, never derived from geometry.
/// - `drawn` is the sum of the item shapes, the shapes that each stand for one item. A
///   group shape is not in it, and a shape nested inside a drawn folder lies within that
///   folder's value and is never added again. A format whose shapes are all groups, such
///   as the histogram, has no item shape: its `drawn` total is zero.
/// - `grouped` is the sum of every group, whether the group has a shape or only a table
///   row. For the histogram it is the sum of the items in its intervals.
/// - `filtered_out` is the sum of what the request's condition removed.
///
/// The three sums never overlap. What none of them holds is the `remainder`.
///
/// [`VizModel::validate_revision`] adds the five values only when they are all exact and
/// in one unit, so the totals of a model that counts items are never added to bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VizTotals {
    pub scope: VizTotal,
    pub drawn: VizTotal,
    pub grouped: VizTotal,
    pub filtered_out: VizTotal,
    /// What the scope holds beyond `drawn`, `grouped` and `filteredOut`.
    ///
    /// Siblings below the minimum area or coordinate precision, and space no single item
    /// can carry, such as a file shared between siblings that the scope counts once. The
    /// engine computes it; a consumer only displays it. Exact only when the scope and the
    /// other three totals are exact, and then never negative; otherwise unknown with a
    /// reason. Omitted rather than null; a model without it says nothing about it.
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub remainder: Option<VizTotal>,
    /// What the remainder holds: `viz.remainder.too_small` or `viz.remainder.unattributed`.
    ///
    /// Omitted when the remainder is absent or exactly zero, at least one key when it is
    /// exact and not zero, and the known causes or nothing when it is unknown (its reason
    /// says why). Valid only with `remainder`; keys are not empty, sorted and without
    /// duplicates, and an unregistered key is preserved. Omitted rather than null.
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub remainder_reason_keys: Option<Vec<String>>,
    /// The F03 measurement limits of `scope` (#351): the `measurementLimits` that
    /// `space.total` gives the same item in the model's size basis, from the same
    /// function, so the two always agree. A model that does not measure size has none.
    ///
    /// Only the scope total has them: the other totals are sums F06 makes, which do
    /// not inherit F03's limits. Omitted when there is none, never empty or null; keys
    /// are not empty or repeated, keep F03's order, and an unknown key is preserved.
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub scope_measurement_limits: Option<Vec<String>>,
}

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
        self.validate_scope_limits()?;
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

    /// Present only with a key, and no key empty or repeated (#351).
    fn validate_scope_limits(&self) -> Result<(), &'static str> {
        let Some(keys) = &self.scope_measurement_limits else {
            return Ok(());
        };
        let mut seen = std::collections::HashSet::new();
        if keys.is_empty() || keys.iter().any(|key| key.is_empty() || !seen.insert(key)) {
            return Err("invalid scope measurement limits");
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
