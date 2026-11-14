//! Private full-result candidates and compare-and-replace publication.
use crate::{BuildError, BuildHandle, IngestSink, ResultOrigin, ResultVersion};
use std::sync::Arc;

impl IngestSink {
    /// Opens a private full-replacement candidate based on the exact current result.
    ///
    /// The candidate does not reserve the lineage or publish intermediate snapshots.
    /// Sealing it succeeds only while `base` remains current. Readers that already pinned
    /// `base` keep their immutable snapshot after a successful replacement.
    pub fn open_replacement(&self, base: &Arc<ResultVersion>) -> Result<BuildHandle, BuildError> {
        let lineage_id = base.lineage_id.clone().ok_or(BuildError::BaseSuperseded)?;
        let lineage = Some(lineage_id);
        let baseline = self
            .state
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .begin_replacement(&lineage, &base.version_id)?;
        if !Arc::ptr_eq(&baseline.result, base) {
            return Err(BuildError::BaseSuperseded);
        }
        Ok(BuildHandle {
            state: Arc::clone(&self.state),
            events: Arc::clone(&self.events),
            baseline: Some(baseline),
            lineage,
            expected_base: Some(base.version_id.clone()),
            reserved: false,
            published: false,
            origin: ResultOrigin::Incremental,
            data_source: base.data_source,
            size_rule: base.size_rule,
            shared_table_limit: base.shared_table_limit,
            scope_version_id: base.scope_version_id.clone(),
            scope_fingerprint: base.scope_fingerprint.clone(),
            roots: base.roots.clone(),
            gaps: Vec::new(),
            observations: Default::default(),
            directory_case: Default::default(),
            issues: Vec::new(),
            volume_reports: Vec::new(),
            finished: false,
        })
    }
}
