//! Frozen scope evidence in every publication of one scan build.
//!
//! FR1.HD1.app-host.DD1: a result carries the scope version ID and the scope
//! fingerprint so that readers can confirm the conditions of the request.
use crate::{BuildError, BuildHandle, IngestSink, ResultVersion};
use du_core_contracts::{Id, result::ResultScopeRoot};

/// The scope version and fingerprint fixed when collection started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeHeader {
    /// Identifier of the frozen scope version.
    pub version_id: Id,
    /// Canonical fingerprint of the same frozen scope conditions.
    pub fingerprint: String,
}

impl IngestSink {
    /// Opens a scan forest whose result header carries the frozen scope.
    ///
    /// Every partial and sealed publication of the build repeats the same
    /// header, and an incremental replacement keeps it. `lineage` follows
    /// [`IngestSink::open_forest_in_lineage`]. An empty fingerprint or root
    /// list is rejected with [`BuildError::InvalidTree`] before any reservation.
    pub fn open_scoped_forest(
        &self,
        scope: ScopeHeader,
        roots: Vec<ResultScopeRoot>,
        lineage: Option<Id>,
    ) -> Result<BuildHandle, BuildError> {
        if scope.fingerprint.is_empty() {
            return Err(BuildError::InvalidTree);
        }
        let mut build = match lineage {
            Some(lineage) => self.open_forest_in_lineage(scope.version_id, roots, lineage)?,
            None => self.open_forest(scope.version_id, roots)?,
        };
        build.scope_fingerprint = Some(scope.fingerprint);
        Ok(build)
    }
}

impl ResultVersion {
    /// Fingerprint of the frozen scope conditions, when the producer bound one.
    ///
    /// `None` means the result was opened without a scope header; it is not
    /// evidence of any particular scope.
    pub fn scope_fingerprint(&self) -> Option<&str> {
        self.scope_fingerprint.as_deref()
    }
}
