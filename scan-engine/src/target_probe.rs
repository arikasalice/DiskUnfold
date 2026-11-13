//! Target-probe job description and result conversion.
use du_core_contracts::{
    OsPath, Timestamp,
    analysis_ipc::{TargetAccess, TargetStatusChanged},
    job::{HistoryMode, JobControl, JobDescriptor, ProgressUnit, ResourceClass},
};

use crate::target::{AccessStatus, ProbeResult};

/// Job kind whose item payloads are JSON-encoded `TargetStatusChanged` values.
pub const TARGET_PROBE_KIND: &str = "target.probe";

/// Describes the read-only, cancellable target-probe job.
pub fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: TARGET_PROBE_KIND.into(),
        version: 1,
        display_name_key: "job.kind.target_probe".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["target".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::MetadataIo],
        history: HistoryMode::None,
    }
}

/// Converts one completed probe without manufacturing capacity or identity.
/// An absent or unrepresentable observation time cannot form an item event.
pub fn status(target: OsPath, result: ProbeResult) -> Option<TargetStatusChanged> {
    let access = match result.access.status {
        AccessStatus::Accessible => TargetAccess::Accessible,
        AccessStatus::Denied => TargetAccess::Denied,
        AccessStatus::Disconnected => TargetAccess::Disconnected,
        AccessStatus::Missing => TargetAccess::Missing,
        AccessStatus::Unresponsive => TargetAccess::Unresponsive,
        AccessStatus::Unknown => TargetAccess::Unknown,
    };
    let (total_bytes, available_bytes, capacity_reason) = match result.capacity {
        Ok(capacity) => (
            Some(capacity.total_bytes.into()),
            Some(capacity.available_bytes.into()),
            None,
        ),
        Err(reason) => (None, None, Some(reason)),
    };
    Some(TargetStatusChanged {
        target,
        access,
        reason_key: result.access.reason,
        total_bytes,
        available_bytes,
        capacity_reason,
        aggregation_uncertain: result.aggregation_uncertain,
        observed_at: Timestamp::from_unix_millis(result.observed_at?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::AccessResult;
    use du_core_contracts::{PathBytes, PathEncoding, volume::VolumeCapacity};

    fn path() -> OsPath {
        OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[67, 0, 58, 0, 92, 0]),
            display: "C:\\".into(),
            lossy: false,
        }
    }

    #[test]
    fn descriptor_is_cancellable_read_only_and_unrecorded() {
        let descriptor = descriptor();
        assert_eq!(descriptor.kind_id, "target.probe");
        assert_eq!(descriptor.controls, [JobControl::Cancel]);
        assert_eq!(descriptor.history, HistoryMode::None);
        assert_eq!(descriptor.resource_classes, [ResourceClass::MetadataIo]);
    }

    #[test]
    fn converts_capacity_and_preserves_unavailable_reason() -> Result<(), &'static str> {
        let success = status(
            path(),
            ProbeResult {
                access: AccessResult {
                    status: AccessStatus::Accessible,
                    reason: None,
                },
                capacity: Ok(VolumeCapacity {
                    total_bytes: 100,
                    available_bytes: 20,
                }),
                aggregation_uncertain: true,
                observed_at: Some(1),
            },
        )
        .ok_or("missing successful observation")?;
        assert_eq!(success.access, TargetAccess::Accessible);
        assert_eq!(success.observed_at.as_str(), "1970-01-01T00:00:00.001Z");
        assert_eq!(success.total_bytes, Some(100u64.into()));
        assert!(success.aggregation_uncertain);
        let denied = status(
            path(),
            ProbeResult {
                access: AccessResult {
                    status: AccessStatus::Denied,
                    reason: Some("scan.path_denied".into()),
                },
                capacity: Err("scan.path_denied".into()),
                aggregation_uncertain: true,
                observed_at: Some(2),
            },
        )
        .ok_or("missing denied observation")?;
        assert_eq!(denied.access, TargetAccess::Denied);
        assert_eq!(denied.total_bytes, None);
        assert_eq!(denied.capacity_reason.as_deref(), Some("scan.path_denied"));
        Ok(())
    }
}
