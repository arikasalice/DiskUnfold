//! Registry-backed implementation of the F13 message resolution port.

use std::{collections::BTreeMap, sync::Arc};

use du_core_contracts::{
    Timestamp,
    locale_bundle::{LocaleDatePattern, LocaleFormatTable, LocaleMessageValue},
    locale_message::{LocaleMessagePort, LocaleMessageRequest, RenderedLocaleMessage},
};

use super::Registry;
use crate::locale::{
    Decimal, FormatProfile, FormatTableView, FormatText, RawValue, ValueState, format_raw,
};

/// Language whose format table serves a language that has no registered bundle.
const BASELINE_LANGUAGE: &str = "en";

/// Display preferences applied to counts, sizes, timestamps and numbers.
///
/// A port takes these once at construction. A settings change builds a new port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocaleFormatSettings {
    /// Number format identifier, for example `lang_default` or `no_grouping`.
    pub number_format_id: String,
    /// Date format identifier, for example `lang_default` or `iso8601`.
    pub date_format_id: String,
    /// Time zone policy, `local` or `utc`.
    pub time_zone_policy: String,
    /// Size unit system, `si` or `iec`.
    pub size_unit_system: String,
    /// Selected size unit or `auto`.
    pub size_unit: String,
    /// Observed local UTC offset in seconds, used only by the `local` policy.
    pub local_offset_seconds: Option<i32>,
}

/// Owned copy of one bundle's format table, validated at registration.
struct OwnedFormatTable {
    decimal_separator: String,
    grouping_separator: String,
    grouping_size: usize,
    date_patterns: BTreeMap<String, String>,
    unit_symbols: BTreeMap<String, String>,
}

impl OwnedFormatTable {
    fn from_bundle(value: &serde_json::Value) -> Option<Self> {
        let table: LocaleFormatTable = serde_json::from_value(value.clone()).ok()?;
        Some(Self {
            grouping_size: usize::try_from(table.grouping_size).ok()?,
            decimal_separator: table.decimal_separator,
            grouping_separator: table.grouping_separator,
            date_patterns: table
                .date_patterns
                .into_iter()
                .map(|(id, pattern)| (date_pattern_id(id).to_owned(), pattern))
                .collect(),
            unit_symbols: table.unit_symbols,
        })
    }

    fn view(&self) -> FormatTableView<'_> {
        FormatTableView {
            decimal_separator: &self.decimal_separator,
            grouping_separator: &self.grouping_separator,
            grouping_size: self.grouping_size,
            date_patterns: &self.date_patterns,
            unit_symbols: &self.unit_symbols,
        }
    }
}

const fn date_pattern_id(id: LocaleDatePattern) -> &'static str {
    match id {
        LocaleDatePattern::LangDefault => "lang_default",
        LocaleDatePattern::Iso8601 => "iso8601",
    }
}

/// Message port backed by the verified startup [`Registry`].
///
/// Message selection, fallback and parameter checks follow
/// [`Registry::render_text`]. Values are formatted with this crate's
/// [`format_raw`] using the format table of the requested language, or of
/// English when that language has no registered bundle. Rendering a message
/// that falls back to English still formats numbers and dates for the
/// requested language. The port is immutable after construction, so it can be
/// shared across threads.
pub struct RegistryMessagePort {
    registry: Arc<Registry>,
    settings: LocaleFormatSettings,
    tables: BTreeMap<String, OwnedFormatTable>,
}

impl RegistryMessagePort {
    /// Builds a port over a registered bundle set and the active format settings.
    ///
    /// A bundle whose format table cannot be read is left without one; values
    /// that need formatting then render as `value.unknown`. Startup
    /// registration already rejects such tables, so this is a defensive path.
    pub fn new(registry: Arc<Registry>, settings: LocaleFormatSettings) -> Self {
        let tables = registry
            .languages()
            .filter_map(|language| {
                let bundle = registry.bundle(language)?;
                let table = OwnedFormatTable::from_bundle(&bundle.format_table)?;
                Some((language.to_owned(), table))
            })
            .collect();
        Self {
            registry,
            settings,
            tables,
        }
    }

    fn table_for(&self, language: &str) -> Option<&OwnedFormatTable> {
        self.tables
            .get(language)
            .or_else(|| self.tables.get(BASELINE_LANGUAGE))
    }

    /// Formats one numeric value, or returns `None` when it has no valid display form.
    fn format_value(&self, language: &str, value: LocaleMessageValue<'_>) -> Option<String> {
        let (format_id, raw) = match value {
            LocaleMessageValue::Count(count) => {
                ("count", RawValue::Count(ValueState::Exact(count)))
            }
            LocaleMessageValue::Size(bytes) => {
                ("capacity", RawValue::Capacity(ValueState::Exact(bytes)))
            }
            LocaleMessageValue::Timestamp(text) => (
                "time",
                RawValue::Time(ValueState::Exact(Timestamp::parse(text).ok()?)),
            ),
            LocaleMessageValue::Number { coefficient, scale } => (
                "number",
                RawValue::Number(ValueState::Exact(Decimal::new(coefficient, scale)?)),
            ),
            // The resolver inserts these itself; they never reach a formatter.
            LocaleMessageValue::Path(_)
            | LocaleMessageValue::Name(_)
            | LocaleMessageValue::MessageKey(_) => return None,
        };
        let table = self.table_for(language)?;
        let profile = FormatProfile {
            number_format_id: &self.settings.number_format_id,
            date_format_id: &self.settings.date_format_id,
            time_zone_policy: &self.settings.time_zone_policy,
            size_unit_system: &self.settings.size_unit_system,
            size_unit: &self.settings.size_unit,
        };
        let output = format_raw(
            format_id,
            &raw,
            &profile,
            &table.view(),
            self.settings.local_offset_seconds,
        );
        match output.visual {
            FormatText::Literal(text) => Some(text),
            FormatText::Message { .. } => None,
        }
    }
}

impl LocaleMessagePort for RegistryMessagePort {
    fn render(&self, request: &LocaleMessageRequest<'_>) -> RenderedLocaleMessage {
        let rendered =
            self.registry
                .render_text(request.language, request.key, request.parameters, |value| {
                    self.format_value(request.language, value)
                });
        RenderedLocaleMessage {
            text: rendered.text,
            marker: rendered.marker,
            unknown_parameter: rendered.unknown_parameter,
        }
    }
}

#[cfg(test)]
mod tests;
