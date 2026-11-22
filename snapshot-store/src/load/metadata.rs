use super::LoadError;
use du_analysis_index::{VolumeReport, VolumeReportPoint};
use du_core_contracts::{
    Timestamp,
    ports::{PortError, PortErrorKind},
    scan_observation::VolumeSpace,
};

pub(super) fn volume_reports(
    volumes: &[crate::records::volume::VolumeEntry<'_>],
) -> Result<Vec<VolumeReport>, LoadError> {
    volumes
        .iter()
        .map(|volume| {
            Ok(VolumeReport {
                volume_id: volume.id.to_vec(),
                start: volume_point(&volume.start)?,
                end: volume_point(&volume.end)?,
            })
        })
        .collect()
}

fn volume_point(
    observation: &crate::records::volume::Observation<'_>,
) -> Result<Option<VolumeReportPoint>, LoadError> {
    match observation {
        crate::records::volume::Observation::NotAttempted => Ok(None),
        crate::records::volume::Observation::Success {
            observed_ms,
            total,
            used,
            free,
        } => Ok(Some(VolumeReportPoint {
            observed_at: timestamp(*observed_ms)?,
            value: Ok(VolumeSpace {
                total_bytes: *total,
                used_bytes: *used,
                free_bytes: *free,
            }),
        })),
        crate::records::volume::Observation::Failure {
            observed_ms,
            kind,
            reason,
            os_code,
        } => Ok(Some(VolumeReportPoint {
            observed_at: timestamp(*observed_ms)?,
            value: Err(PortError {
                kind: port_error_kind(kind),
                reason_key: (*reason).to_owned(),
                os_code: *os_code,
            }),
        })),
    }
}

fn port_error_kind(kind: &str) -> PortErrorKind {
    match kind {
        "not_found" => PortErrorKind::NotFound,
        "permission_denied" => PortErrorKind::PermissionDenied,
        "in_use" => PortErrorKind::InUse,
        "unsupported" => PortErrorKind::Unsupported,
        "identity_changed" => PortErrorKind::IdentityChanged,
        "busy" => PortErrorKind::Busy,
        "corrupt" => PortErrorKind::Corrupt,
        "schema_mismatch" => PortErrorKind::SchemaMismatch,
        "io" => PortErrorKind::Io,
        _ => PortErrorKind::Io,
    }
}

pub(super) fn timestamp(value: Option<i64>) -> Result<Option<Timestamp>, LoadError> {
    value
        .map(|value| Timestamp::from_unix_millis(value).ok_or(LoadError::InvalidSnapshot))
        .transpose()
}
