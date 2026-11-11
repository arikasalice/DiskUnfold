//! Ranked row preparation using the shared model publication boundary.
use super::*;

/// Ranked view input prepared from one immutable source before publication.
pub(crate) struct RankedInput<'a> {
    pub parent: &'a ItemRef,
    pub basis: &'a SizeBasis,
    pub fingerprint: String,
    pub layout: Option<projection::Model>,
    pub evidence: Vec<ChildEvidence>,
}

impl ModelPages {
    /// Prepares ranked rows against the same pinned source and publication store.
    /// A missing attributable projection retains the original rows as a fallback.
    pub(crate) fn prepare_ranked(
        &self,
        source: &current_analysis::CurrentAnalysis,
        input: RankedInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<PreparedRender, Error> {
        let RankedInput {
            parent,
            basis,
            fingerprint,
            layout,
            evidence,
        } = input;
        let normalized = normalize(evidence, source.result_version(), cancelled)?;
        let presentations = normalized
            .into_values()
            .map(|child| split_evidence(child).1)
            .collect();
        let input = match &layout {
            Some(model) => production::project_rows(
                source.version().clone(),
                model,
                fingerprint,
                presentations,
                cancelled,
            ),
            None => production::project_fallback(
                source.version().clone(),
                parent,
                basis,
                fingerprint,
                presentations,
                cancelled,
            ),
        }
        .map_err(Error::Registration)?;
        let input =
            prepare_scoped_model_input(input, parent, cancelled).map_err(registration_error)?;
        Ok(PreparedRender {
            owner: Arc::downgrade(&self.identity),
            input,
            fallback_reason: layout
                .is_none()
                .then_some(projection::Error::UnavailableAttribution),
            layout: layout.map(Arc::new),
        })
    }
}
