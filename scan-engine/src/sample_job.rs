//! Job description for loading the built-in sample observation.
use du_core_contracts::job::{HistoryMode, JobControl, JobDescriptor, ProgressUnit, ResourceClass};

/// Job kind that publishes the sample observations of a validated bundle.
pub const SAMPLE_JOB_KIND: &str = "scan.sample";

/// Describes the cancellable sample load job. It touches no storage and leaves no history.
pub fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: SAMPLE_JOB_KIND.into(),
        version: 1,
        display_name_key: "job.kind.sample_load".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["sample.observation_sealed".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::Compute],
        history: HistoryMode::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_cancellable_compute_only_and_unrecorded() {
        let descriptor = descriptor();
        assert_eq!(descriptor.kind_id, "scan.sample");
        assert_eq!(descriptor.version, 1);
        assert_eq!(descriptor.display_name_key, "job.kind.sample_load");
        assert_eq!(descriptor.controls, [JobControl::Cancel]);
        assert_eq!(descriptor.safe_points, ["sample.observation_sealed"]);
        assert_eq!(descriptor.progress_unit, ProgressUnit::Items);
        assert_eq!(descriptor.resource_classes, [ResourceClass::Compute]);
        assert_eq!(descriptor.history, HistoryMode::None);
    }
}
