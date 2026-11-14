//! Sample result versions: session-bound lineages that never become the implicit current result.
use crate::{BuildError, BuildHandle, IngestSink};
use du_core_contracts::{Id, data_source::DataSource, result::ResultScopeRoot};

/// Opaque key that groups the sample builds of one experience session.
///
/// The store compares keys for equality only and never interprets the identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SampleSessionKey(Id);

impl SampleSessionKey {
    /// Wraps a caller-chosen identifier without interpreting it.
    pub fn new(id: Id) -> Self {
        Self(id)
    }
}

impl IngestSink {
    /// Opens a forest build whose publications are sample results bound to `session`.
    ///
    /// Every call gets its own lineage, so several observation-time versions of one session
    /// stay current together. Sample versions are reachable only by exact version ID
    /// (`pin_current`, `pin_scan` with a revision), never through `pin()` or an
    /// unqualified `pin_scan`, and their publication sends no result-version notice.
    /// Roots are validated as in [`IngestSink::open_forest`].
    pub fn open_sample_forest(
        &self,
        scope_version_id: Id,
        roots: Vec<ResultScopeRoot>,
        session: &SampleSessionKey,
    ) -> Result<BuildHandle, BuildError> {
        if roots.is_empty() || u32::try_from(roots.len()).is_err() {
            return Err(BuildError::InvalidTree);
        }
        let lineage = self
            .state
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .register_sample_lineage(session)?;
        match self.begin(scope_version_id, roots, Some(lineage.clone())) {
            Ok(mut handle) => {
                handle.data_source = DataSource::Sample;
                Ok(handle)
            }
            Err(error) => {
                if let Ok(mut state) = self.state.lock() {
                    state.unregister_sample_lineage(&lineage);
                }
                Err(error)
            }
        }
    }

    /// Detaches every current version of `session` from the store and returns how many.
    ///
    /// Readers that already pinned a version keep reading it. Afterwards `pin_current`
    /// rejects those IDs, and a build of the session that is still open fails when it tries
    /// to publish. An unknown or already released session returns 0, so the call is safe to
    /// repeat.
    pub fn release_sample_session(&self, session: &SampleSessionKey) -> Result<u64, BuildError> {
        let released = self
            .state
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .release_sample_session(session);
        // Entries drop here, after the store lock is released.
        Ok(u64::try_from(released.len()).unwrap_or(u64::MAX))
    }
}
