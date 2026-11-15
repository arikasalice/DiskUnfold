//! Interim F06 vocabulary and policy-table revisions for the product producer.
//!
//! Every wire string below is interim, pending standardization review: the F06
//! HLD names these concepts (tables 22 and 32) but no published contract fixes
//! their text yet. They live in this one module so a reviewed rename is one edit.
use std::collections::HashMap;

use du_core_contracts::visualization::model::{VizStaleness, VizStalenessState};

use crate::{
    required_data, viz_catalog,
    viz_runtime::{LegendClass, SourcePolicyFacts},
};

/// The one measure `viz.render` serves.
pub const MEASURE_SIZE: &str = "size";
/// The one denominator basis `viz.render` serves, spelled as its wire value.
pub const DENOMINATOR_SCOPE: &str = "scope";
/// The one color criterion served until the F06 legend rule table (FR6) exists.
///
/// It means that no classification is applied and every item shares one class.
pub const COLOR_CRITERION_NONE: &str = "none";
/// The legend class of every item while no classification rule exists.
pub const LEGEND_CLASS_NONE: &str = "none";
/// Message key naming the class in the legend.
pub const LEGEND_LABEL_NONE: &str = "viz.legend.none";
/// Display flag for a size value whose amount or evidence is unknown.
pub const FLAG_UNKNOWN: &str = "viz.flag.unknown";
/// Display flag for a size value that is only a lower bound.
pub const FLAG_LOWER_BOUND: &str = "viz.flag.lower_bound";
/// Display flag for a grouped (Other) shape.
pub const FLAG_OTHER_GROUP: &str = "viz.flag.other_group";
/// Message key carried as the display name of the Other group.
pub const GROUP_OTHER_NAME_KEY: &str = "viz.group.other";
/// Model-local reference of the Other group.
pub const GROUP_OTHER_REF: &str = "viz.group.other";
/// Prefix of the model-local reference of a histogram interval.
pub const GROUP_BIN_REF_PREFIX: &str = "viz.group.bin.";
/// Reason carried by an unknown byte quantity; F03 uses this key for unknown sizes.
pub const UNKNOWN_SIZE_REASON: &str = "space.data_insufficient";
/// Reason of a match key; F05 browse rows carry the same unavailable correspondence.
pub const MATCH_KEY_REASON: &str = "ref.unavailable";
/// Revision of the interpretation table (HLD table 8); only its size row applies.
pub const INTERPRETATION_TABLE_REVISION: u32 = 1;
/// Revision of the interim single-class legend rule; FR6 replaces it with the rule table.
pub const LEGEND_RULE_REVISION: u32 = 1;

/// The catalog policy the product advertises; it lists only what render serves.
pub fn catalog_policy() -> viz_catalog::Policy {
    viz_catalog::Policy {
        measures: vec![MEASURE_SIZE.into()],
        color_criteria: vec![COLOR_CRITERION_NONE.into()],
        denominator_criteria: vec![DENOMINATOR_SCOPE.into()],
        interpretation_table_revision: INTERPRETATION_TABLE_REVISION,
        required_data_table_revision: required_data::TABLE_REVISION,
        legend_rule_revision: LEGEND_RULE_REVISION,
    }
}

/// The owner-bound facts of a pinned result; the F03 rule is taken from the result.
///
/// The pinned result is the current result of its lineage, so it is `Current`.
pub(super) fn source_policy(
    view_policy_revision: u32,
    available_formats: Vec<String>,
) -> SourcePolicyFacts {
    SourcePolicyFacts {
        staleness: VizStaleness {
            state: VizStalenessState::Current,
            reason_key: None,
        },
        interpretation_revision: INTERPRETATION_TABLE_REVISION,
        required_data_revision: required_data::TABLE_REVISION,
        legend_revision: LEGEND_RULE_REVISION,
        view_policy_revision,
        classes: HashMap::from([(
            LEGEND_CLASS_NONE.to_owned(),
            LegendClass {
                label_key: LEGEND_LABEL_NONE.to_owned(),
                condition: None,
            },
        )]),
        available_formats,
    }
}
