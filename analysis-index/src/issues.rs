//! Classified scan facts owned by each immutable publication.
use crate::{BuildError, BuildHandle, ResultVersion};
use du_core_contracts::scan_failures::ScanIssue;

impl BuildHandle {
    /// Records a classified observation using its original root-relative bytes.
    pub fn record_scan_issue(&mut self, issue: ScanIssue) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        if !self.valid_root(issue.root_index)
            || issue.relative.iter().any(Vec::is_empty)
            || issue.reason_key.is_empty()
        {
            return Err(BuildError::InvalidTree);
        }
        self.issues.push(issue);
        Ok(())
    }
}

impl ResultVersion {
    /// Returns the deterministically ordered collection facts pinned to this version.
    pub fn scan_issues(&self) -> &[ScanIssue] {
        &self.issues
    }
}

pub(crate) fn ordered(issues: &[ScanIssue]) -> Vec<ScanIssue> {
    let mut result = issues.to_vec();
    result.sort_by(|a, b| {
        (
            a.root_index,
            &a.relative,
            a.kind,
            a.stage,
            &a.reason_key,
            a.observed_at.as_ref().map(|t| t.as_str()),
        )
            .cmp(&(
                b.root_index,
                &b.relative,
                b.kind,
                b.stage,
                &b.reason_key,
                b.observed_at.as_ref().map(|t| t.as_str()),
            ))
    });
    result
}
