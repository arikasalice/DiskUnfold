//! F03 unit rule table shared by native and web formatters.

/// Version one uses a single decimal place and successive powers of the base.
#[derive(serde::Serialize)]
pub struct UnitTable {
    base: u16,
    symbols: &'static [&'static str],
}

impl UnitTable {
    /// Multiplicative step from one unit to the next.
    pub fn base(&self) -> u16 {
        self.base
    }

    /// Unit symbols, ordered from the byte through the largest displayed unit.
    pub fn symbols(&self) -> &'static [&'static str] {
        self.symbols
    }
}

pub const UNIT_TABLE_REVISION: u32 = 1;
pub const DISPLAY_DECIMAL_PLACES: u32 = 1;
pub const SI_UNITS: UnitTable = UnitTable {
    base: 1000,
    symbols: &["B", "kB", "MB", "GB", "TB", "PB"],
};
pub const IEC_UNITS: UnitTable = UnitTable {
    base: 1024,
    symbols: &["B", "KiB", "MiB", "GiB", "TiB", "PiB"],
};
