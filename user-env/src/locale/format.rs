//! Deterministic raw-value formatting for locale-aware text and speech.

use std::collections::BTreeMap;

use du_core_contracts::Timestamp;
use du_core_contracts::size_units::{DISPLAY_DECIMAL_PLACES, IEC_UNITS, SI_UNITS};

/// A decimal number represented without floating-point conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decimal {
    coefficient: i128,
    scale: u8,
}

impl Decimal {
    /// Builds a decimal whose scale is supported by the fixed-point formatter.
    pub fn new(coefficient: i128, scale: u8) -> Option<Self> {
        (scale <= 18).then_some(Self { coefficient, scale })
    }
}

/// Completeness of one observed raw value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueState<T> {
    Exact(T),
    LowerBound(T),
    Unknown { reason_key: String },
}

/// The four raw kinds handled by F13's ordered formatting table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawValue {
    Count(ValueState<u64>),
    Number(ValueState<Decimal>),
    Capacity(ValueState<u64>),
    Time(ValueState<Timestamp>),
}

impl RawValue {
    fn kind_id(&self) -> &'static str {
        match self {
            Self::Count(_) => "count",
            Self::Number(_) => "number",
            Self::Capacity(_) => "capacity",
            Self::Time(_) => "time",
        }
    }
}

/// Borrowed, already-validated fields from a locale bundle's format table.
///
/// The caller adapts `core_contracts::locale_bundle::LocaleFormatTable` here;
/// this module owns formatting policy and does not parse a bundle or perform I/O.
#[derive(Clone, Copy)]
pub struct FormatTableView<'a> {
    pub decimal_separator: &'a str,
    pub grouping_separator: &'a str,
    pub grouping_size: usize,
    pub date_patterns: &'a BTreeMap<String, String>,
    pub unit_symbols: &'a BTreeMap<String, String>,
}

/// The active format IDs and F03 display-unit selections.
#[derive(Clone, Copy)]
pub struct FormatProfile<'a> {
    pub number_format_id: &'a str,
    pub date_format_id: &'a str,
    pub time_zone_policy: &'a str,
    pub size_unit_system: &'a str,
    pub size_unit: &'a str,
}

/// Display text is either a literal or a message that the caller resolves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatText {
    Literal(String),
    Message {
        key: &'static str,
        parameters: BTreeMap<String, String>,
    },
}

/// One message key and its string-valued parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatMarker {
    pub key: &'static str,
    pub parameters: BTreeMap<String, String>,
}

/// Structured speech data for the verified message resolver at the consumer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenReaderOutput {
    Plain(String),
    Quantity {
        number: String,
        unit_id: String,
    },
    Time {
        date: String,
        offset_seconds: i32,
    },
    Message {
        key: &'static str,
        parameters: BTreeMap<String, String>,
    },
}

/// A formatted value, unresolved message markers, and diagnostic keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatOutput {
    pub visual: FormatText,
    pub screen_reader: ScreenReaderOutput,
    pub markers: Vec<FormatMarker>,
    pub unknown_reason: Option<String>,
    pub warnings: Vec<&'static str>,
}

struct FormatRequest<'a> {
    raw: &'a RawValue,
    profile: FormatProfile<'a>,
    table: FormatTableView<'a>,
    local_offset_seconds: Option<i32>,
}

type Handler = for<'a> fn(&FormatRequest<'a>) -> FormatOutput;

struct HandlerRow {
    id: &'static str,
    apply: Handler,
}

const HANDLERS: [HandlerRow; 4] = [
    HandlerRow {
        id: "count",
        apply: numeric::format_count,
    },
    HandlerRow {
        id: "number",
        apply: numeric::format_number_value,
    },
    HandlerRow {
        id: "capacity",
        apply: numeric::format_capacity,
    },
    HandlerRow {
        id: "time",
        apply: time::format_time,
    },
];

/// Formats a raw value using one table row selected by `format_id`.
///
/// An unknown or mismatched kind ID falls back to the raw value's own row and
/// records `locale.format_unknown`. Local time uses only the supplied offset
/// observation; this function never asks the operating system for time-zone data.
pub fn format_raw(
    format_id: &str,
    raw: &RawValue,
    profile: &FormatProfile<'_>,
    table: &FormatTableView<'_>,
    local_offset_seconds: Option<i32>,
) -> FormatOutput {
    let request = FormatRequest {
        raw,
        profile: *profile,
        table: *table,
        local_offset_seconds,
    };
    let row = HANDLERS.iter().find(|row| row.id == format_id);
    let matches_kind = row.is_some_and(|row| row.id == raw.kind_id());
    let selected = if matches_kind {
        row
    } else {
        HANDLERS.iter().find(|row| row.id == raw.kind_id())
    };
    let mut output = selected.map_or_else(unknown_kind_output, |row| (row.apply)(&request));
    if !matches_kind {
        output.warnings.push("locale.format_unknown");
    }
    output
}

mod numeric;
mod time;

fn known_output(
    visual: String,
    screen_reader: ScreenReaderOutput,
    lower_bound: bool,
) -> FormatOutput {
    let markers = if lower_bound {
        vec![FormatMarker {
            key: "value.lower_bound",
            parameters: BTreeMap::from([("value".to_owned(), visual.clone())]),
        }]
    } else {
        Vec::new()
    };
    FormatOutput {
        visual: FormatText::Literal(visual),
        screen_reader,
        markers,
        unknown_reason: None,
        warnings: Vec::new(),
    }
}

fn unknown_output(reason_key: String) -> FormatOutput {
    let parameters = BTreeMap::from([("reasonKey".to_owned(), reason_key.clone())]);
    let message = FormatText::Message {
        key: "value.unknown",
        parameters: parameters.clone(),
    };
    FormatOutput {
        visual: message,
        screen_reader: ScreenReaderOutput::Message {
            key: "value.unknown",
            parameters: parameters.clone(),
        },
        markers: vec![FormatMarker {
            key: "value.unknown",
            parameters,
        }],
        unknown_reason: Some(reason_key),
        warnings: Vec::new(),
    }
}

fn unknown_kind_output() -> FormatOutput {
    unknown_output("locale.format_unknown".to_owned())
}

#[cfg(test)]
mod tests;
