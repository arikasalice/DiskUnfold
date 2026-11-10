//! Volume space evidence retained independently of item aggregates.
use crate::{BuildError, BuildHandle};
use du_core_contracts::Timestamp;

/// A volume observation and the optional clock value recorded at the attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeReportPoint {
    pub observed_at: Option<Timestamp>,
    pub value: Result<
        du_core_contracts::scan_observation::VolumeSpace,
        du_core_contracts::ports::PortError,
    >,
}

/// Distinct start and end attempts for one scope volume.
/// An empty identity represents an unknown root volume, never a shared identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeReport {
    pub volume_id: Vec<u8>,
    /// None means no observation was attempted.
    pub start: Option<VolumeReportPoint>,
    /// None means no observation was attempted.
    pub end: Option<VolumeReportPoint>,
}

impl BuildHandle {
    /// Root volume identities in the producer's original order.
    pub fn root_volumes(&self) -> Vec<Vec<u8>> {
        self.roots
            .iter()
            .map(|root| root.volume_id.clone())
            .collect()
    }

    /// Replaces volume evidence for the next immutable publication.
    pub fn set_volume_reports(&mut self, reports: Vec<VolumeReport>) -> Result<(), BuildError> {
        if self.finished {
            return Err(BuildError::Unavailable);
        }
        let mut seen = std::collections::HashSet::new();
        for report in &reports {
            if !self
                .roots
                .iter()
                .any(|root| root.volume_id == report.volume_id)
                || (!report.volume_id.is_empty() && !seen.insert(&report.volume_id))
            {
                return Err(BuildError::InvalidTree);
            }
        }
        self.volume_reports = reports;
        Ok(())
    }
}
