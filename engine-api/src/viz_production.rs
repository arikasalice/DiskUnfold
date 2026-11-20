//! Product evidence for the F06 visualization runtime, read from the pinned F03 result.
//!
//! [`ProductionEvidence`] answers the runtime's owner questions from the immutable
//! result it is given: size values, their evidence and value state come from F03
//! unchanged, and nothing is derived from display names or the clock. The one fact
//! read from a name is its type, which the type classification table in the
//! analysis index decides from the native name bytes. Facts the pinned result
//! cannot prove stay absent or unknown.
mod facts;
mod groups;
pub mod policy;
mod presentation;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use du_core_contracts::{
    ItemRef, result::ResultItem, size_units::UnitTable, visualization::request::VizRenderRequest,
};

use crate::{
    current_analysis::CurrentAnalysis,
    error::EngineError,
    required_data::FactValues,
    viz_pages::ranked_group::OtherPresentation,
    viz_runtime::{EvidenceProducer, HistogramFacts, ItemEvidence, SourcePolicyFacts},
};

/// Supplies the F03 unit table (SI or IEC) selected by the display setting.
///
/// It is read when a histogram request is admitted, so a later setting change
/// applies to the next request and never alters a model already issued.
pub type UnitTableSource = Arc<dyn Fn() -> &'static UnitTable + Send + Sync>;

/// Reads F06 evidence for the live scan results of one desktop session.
pub struct ProductionEvidence {
    formats: Vec<String>,
    view_policy_revision: u32,
    unit_table: UnitTableSource,
    folder_facts: facts::FolderFacts,
}

impl ProductionEvidence {
    /// Creates the producer for the registered catalog `formats`.
    ///
    /// `view_policy_revision` must be the revision of the view policy registered
    /// with the same runtime; the runtime rejects evidence that disagrees with it.
    pub fn new(
        formats: Vec<String>,
        view_policy_revision: u32,
        unit_table: UnitTableSource,
    ) -> Self {
        Self {
            formats,
            view_policy_revision,
            unit_table,
            folder_facts: facts::FolderFacts::new(),
        }
    }
}

impl EvidenceProducer for ProductionEvidence {
    fn supports_required_data_availability(&self) -> bool {
        true
    }

    fn required_data_facts_for_scope(
        &self,
        source: &CurrentAnalysis,
        aggregate_scope_ref: &ItemRef,
    ) -> Result<FactValues, EngineError> {
        self.folder_facts
            .read(source, aggregate_scope_ref, &facts::AVAILABILITY_BASIS)
    }

    fn required_data_facts(
        &self,
        source: &CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<Option<FactValues>, EngineError> {
        self.folder_facts
            .read(source, &request.aggregate_scope_ref, &request.size_basis)
            .map(Some)
    }

    fn source_policy_facts(
        &self,
        source: &CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<Option<SourcePolicyFacts>, EngineError> {
        // The classification criterion names a rule table that does not exist yet;
        // the color criterion is none or the file type (FR6).
        if request.classification_criterion != policy::CLASSIFICATION_CRITERION_NONE
            || ![policy::COLOR_CRITERION_NONE, policy::COLOR_CRITERION_TYPE]
                .contains(&request.color_criterion.as_str())
        {
            return Err(EngineError::reason("viz.measure_unknown"));
        }
        let values =
            self.folder_facts
                .read(source, &request.aggregate_scope_ref, &request.size_basis)?;
        let available =
            facts::available_formats(source, &request.aggregate_scope_ref, values, &self.formats)?;
        Ok(Some(policy::source_policy(
            self.view_policy_revision,
            available,
            &request.color_criterion,
        )))
    }

    fn ranked_other(
        &self,
        _source: &CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<Option<OtherPresentation>, EngineError> {
        Ok(groups::other_group(&request.format))
    }

    fn histogram_facts(
        &self,
        _source: &CurrentAnalysis,
        _request: &VizRenderRequest,
    ) -> Result<Option<HistogramFacts>, EngineError> {
        Ok(Some(groups::histogram_facts((self.unit_table)())))
    }

    fn child(
        &self,
        source: &CurrentAnalysis,
        item: &ResultItem,
        request: &VizRenderRequest,
    ) -> Result<ItemEvidence, EngineError> {
        Ok(presentation::child(
            source,
            item,
            &request.size_basis,
            request.color_criterion == policy::COLOR_CRITERION_TYPE,
        ))
    }
}
