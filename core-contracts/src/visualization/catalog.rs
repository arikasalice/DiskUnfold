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
#[serde(rename_all = "camelCase")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog_accepts_and_its_request_rejects_unknown_fields() -> Result<(), serde_json::Error> {
        let catalog = json!({"formats":["treemap"],"strategies":[{"id":"treemap","revision":1}],"measures":["size"],"colorCriteria":[],"denominatorCriteria":[],"interpretationTableRevision":1,"requiredDataTableRevision":1,"legendRuleRevision":1});
        let mut extended = catalog.clone();
        extended["futureField"] = json!(true);
        let parsed: VizCatalog = serde_json::from_value(extended)?;
        assert_eq!(serde_json::to_value(parsed)?, catalog);
        serde_json::from_value::<VizCatalogRequest>(json!({}))?;
        assert!(serde_json::from_value::<VizCatalogRequest>(json!({"futureField": true})).is_err());
        Ok(())
    }
}
