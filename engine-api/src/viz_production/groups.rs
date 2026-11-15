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
        legend_class_key: policy::LEGEND_CLASS_NONE.to_owned(),
        display_flags: vec![policy::FLAG_OTHER_GROUP.to_owned()],
    })
}

/// Histogram policy for the selected unit table.
///
/// Each interval starts at one unit of the table, so its name is that unit's symbol.
pub(super) fn histogram_facts(table: &'static UnitTable) -> HistogramFacts {
    HistogramFacts {
        unit_table: table,
        unit_table_revision: du_core_contracts::size_units::UNIT_TABLE_REVISION,
        bins: table
            .symbols()
            .iter()
            .enumerate()
            .map(|(index, symbol)| BinPresentation {
                group_ref: format!("{}{index}", policy::GROUP_BIN_REF_PREFIX),
                display_name: (*symbol).to_owned(),
                legend_class_key: policy::LEGEND_CLASS_NONE.to_owned(),
                display_flags: Vec::new(),
            })
            .collect(),
        unknown_reason: policy::UNKNOWN_SIZE_REASON.to_owned(),
    }
}
