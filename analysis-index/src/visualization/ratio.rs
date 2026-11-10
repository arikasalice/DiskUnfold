//! Exact-input ratios for the published visualization row contract.

use du_core_contracts::visualization::quantity::{VizAmount, VizQuantity};
use std::num::NonZeroU64;

const FRACTION_DIGITS: usize = 20;
const FRACTION_SCALE: u128 = 100_000_000_000_000_000_000;

/// Returns a canonical decimal ratio for exact, same-unit quantities.
///
/// The denominator must be positive. Rounding is half away from zero at
/// twenty fractional places. Decimal long division keeps each intermediate
/// within range for the entire signed visualization quantity domain.
pub fn calculate(numerator: &VizQuantity, denominator: &VizQuantity) -> Option<String> {
    if numerator.unit() != denominator.unit() {
        return None;
    }
    let (VizAmount::Exact { value: numerator }, VizAmount::Exact { value: denominator }) =
        (numerator.amount(), denominator.amount())
    else {
        return None;
    };
    let denominator = u64::try_from(denominator.get())
        .ok()
        .and_then(NonZeroU64::new)?;
    Some(format_ratio(numerator.get(), denominator))
}

/// Formats a ratio whose quantity units and exactness are already established.
pub(super) fn format_ratio(numerator: i128, denominator: NonZeroU64) -> String {
    let denominator = u128::from(denominator.get());
    let magnitude = numerator.unsigned_abs();
    let mut integer = magnitude / denominator;
    let mut remainder = magnitude % denominator;
    let mut fraction = 0;
    for _ in 0..FRACTION_DIGITS {
        remainder *= 10;
        fraction = fraction * 10 + remainder / denominator;
        remainder %= denominator;
    }
    if remainder * 2 >= denominator {
        fraction += 1;
        integer += fraction / FRACTION_SCALE;
        fraction %= FRACTION_SCALE;
    }
    let sign = if numerator < 0 && (integer > 0 || fraction > 0) {
        "-"
    } else {
        ""
    };
    if fraction == 0 {
        return format!("{sign}{integer}");
    }
    let fraction = format!("{fraction:0>FRACTION_DIGITS$}");
    format!("{sign}{integer}.{}", fraction.trim_end_matches('0'))
}
