//! Reads of the current published result and its analysis lineage binding.
use crate::{BuildError, IngestSink, QueryError, ResultVersion};
use du_core_contracts::Id;
use std::sync::Arc;

impl IngestSink {
    /// Pins the current version only if its identifier still matches the admitted request.
    pub fn pin_current(&self, expected: &Id) -> Result<Arc<ResultVersion>, QueryError> {
        let state = self.state.lock().map_err(|_| QueryError::Unavailable)?;
        state.pin_current(expected)
    }
    /// Pins the most recently published current result, when present.
    pub fn pin(&self) -> Result<Option<Arc<ResultVersion>>, BuildError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .pin())
    }
    /// Returns the stable current-analysis lineage for a canonical scope fingerprint.
    /// The caller supplies a unique candidate ID; an existing binding is reused.
    pub fn bind_current_analysis(
        &self,
        fingerprint: &str,
        candidate: Id,
    ) -> Result<Id, BuildError> {
        self.state
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .bind(fingerprint, candidate)
    }
}
