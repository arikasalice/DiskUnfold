//! F06 catalog request and its registered formats, strategies and rule revisions.

use crate::browse_page_ipc::QueryStrategy;
use serde::{Deserialize, Serialize};

/// Reads the catalog for the calling desktop session without selecting a result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "Record<string, never>"))]
#[serde(deny_unknown_fields)]
pub struct VizCatalogRequest {}

/// One snapshot of the layout registry and its associated policy tables.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VizCatalog {
    pub formats: Vec<String>,
    pub strategies: Vec<QueryStrategy>,
    pub measures: Vec<String>,
    pub color_criteria: Vec<String>,
    pub denominator_criteria: Vec<String>,
    pub interpretation_table_revision: u32,
    pub required_data_table_revision: u32,
    pub legend_rule_revision: u32,
}
