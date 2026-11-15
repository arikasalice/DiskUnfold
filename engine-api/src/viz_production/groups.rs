//! Presentation of grouped shapes: the Other group and histogram intervals.
use du_core_contracts::size_units::UnitTable;

use super::policy;
use crate::{
    viz_pages::{histogram::BinPresentation, ranked_group::OtherPresentation},
    viz_runtime::HistogramFacts,
};

/// The Other group of a treemap or bar request.
///
/// The display name is a message key, not text: the user-facing wording follows the
/// localization bundles. Other formats have no Other group.
pub(super) fn other_group(format: &str) -> Option<OtherPresentation> {
    matches!(format, "treemap" | "bar").then(|| OtherPresentation {
        group_ref: policy::GROUP_OTHER_REF.to_owned(),
        display_name: policy::GROUP_OTHER_NAME_KEY.to_owned(),
        legend_class_key: policy::LEGEND_CLASS_UNCLASSIFIED.to_owned(),
        display_flags: vec![policy::FLAG_OTHER_GROUP.to_owned()],
    })
}

/// Histogram policy for the selected unit table.
///
/// Interval names are built from digits, unit symbols and symbols only, so they
/// need no translation: `0 B–1 kB`, `1 kB–1 MB`, … and `≥ 1 PB` for the open last
/// interval. Each boundary is one unit of the table, as the interval rule defines.
pub(super) fn histogram_facts(table: &'static UnitTable) -> HistogramFacts {
    let symbols = table.symbols();
    HistogramFacts {
        unit_table: table,
        unit_table_revision: du_core_contracts::size_units::UNIT_TABLE_REVISION,
        bins: (0..symbols.len())
            .map(|index| BinPresentation {
                group_ref: format!("{}{index}", policy::GROUP_BIN_REF_PREFIX),
                display_name: interval_name(symbols, index),
                legend_class_key: policy::LEGEND_CLASS_UNCLASSIFIED.to_owned(),
                display_flags: Vec::new(),
            })
            .collect(),
        unknown_reason: policy::UNKNOWN_SIZE_REASON.to_owned(),
    }
}

/// Name of the interval that starts at unit `index` (the first starts at zero).
fn interval_name(symbols: &[&str], index: usize) -> String {
    let lower = match (index, symbols.get(index)) {
        (0, Some(symbol)) => format!("0 {symbol}"),
        (_, Some(symbol)) => format!("1 {symbol}"),
        (_, None) => String::new(),
    };
    match symbols.get(index + 1) {
        Some(next) => format!("{lower}\u{2013}1 {next}"),
        None => format!("\u{2265} {lower}"),
    }
}
