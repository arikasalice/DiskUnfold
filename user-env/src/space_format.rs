//! Native text formatting for the shared F03 unit table.

use du_core_contracts::size_units::{DISPLAY_DECIMAL_PLACES, UnitTable};

/// Automatic selection or the index of a unit in the selected table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitSelection {
    Auto,
    Fixed(usize),
}

/// Formats a known byte value without using floating-point arithmetic.
/// A displayed lower bound is rounded downward so it remains a valid bound.
pub fn format_bytes(
    bytes: u64,
    table: &UnitTable,
    selection: UnitSelection,
    lower_bound: bool,
) -> Option<String> {
    let bytes = u128::from(bytes);
    let base = u128::from(table.base());
    let index = match selection {
        UnitSelection::Auto => (1..table.symbols().len())
            .take_while(|index| bytes >= base.pow(*index as u32))
            .last()
            .unwrap_or(0),
        UnitSelection::Fixed(index) => index,
    };
    let symbol = table.symbols().get(index)?;
    if index == 0 {
        return Some(format!("{bytes} {symbol}"));
    }
    let divisor = base.pow(index as u32);
    let scale = 10u128.pow(DISPLAY_DECIMAL_PLACES);
    let rounding = if lower_bound { 0 } else { divisor / 2 };
    let scaled = (bytes * scale + rounding) / divisor;
    let width = DISPLAY_DECIMAL_PLACES as usize;
    Some(format!(
        "{}.{:0width$} {symbol}",
        scaled / scale,
        scaled % scale
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::size_units::{IEC_UNITS, SI_UNITS};

    #[test]
    fn keeps_thresholds_and_rounds_without_overstating_bounds() {
        for (bytes, table, expected) in [
            (0, &SI_UNITS, "0 B"),
            (999, &SI_UNITS, "999 B"),
            (1000, &SI_UNITS, "1.0 kB"),
            (1023, &IEC_UNITS, "1023 B"),
            (1024, &IEC_UNITS, "1.0 KiB"),
            (u64::MAX, &IEC_UNITS, "16384.0 PiB"),
        ] {
            assert_eq!(
                format_bytes(bytes, table, UnitSelection::Auto, false).as_deref(),
                Some(expected)
            );
        }
        assert_eq!(
            format_bytes(1100, &IEC_UNITS, UnitSelection::Fixed(1), false).as_deref(),
            Some("1.1 KiB")
        );
        assert_eq!(
            format_bytes(1100, &IEC_UNITS, UnitSelection::Fixed(1), true).as_deref(),
            Some("1.0 KiB")
        );
        assert_eq!(
            format_bytes(0, &IEC_UNITS, UnitSelection::Fixed(99), false),
            None
        );
    }
}
