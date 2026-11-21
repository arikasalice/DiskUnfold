//! Admits a read-only volume explanation against the current pinned result.
use du_analysis_index::{
    IngestSink, VolumeReportPoint,
    volume_gap::{
        FactorValue, GapRemainder, OtherVolume, PinnedVolumeGap, RemainderUnavailable,
        VolumeGapTarget,
    },
};
use du_core_contracts::{
    PathBytes, SizeEvidence,
    volume::VolumePort,
    volume_gap_ipc::{
        VolumeGapAmount, VolumeGapCategories, VolumeGapObservation, VolumeGapRequest,
        VolumeGapResponse, VolumeGapRow, VolumeGapSize, VolumeGapVolumeRow,
    },
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};
use std::sync::Arc;

const UNAVAILABLE: &str = "space.volume_report_unavailable";
const OUT_OF_SCOPE: &str = "space.target_out_of_scope";

pub(crate) fn register(
    builder: &mut EngineBuilder,
    store: IngestSink,
    volumes: Option<Arc<dyn VolumePort>>,
) {
    builder.register(Query(store, volumes));
}

struct Query(IngestSink, Option<Arc<dyn VolumePort>>);

impl UseCase for Query {
    const NAME: &'static str = "space.volume_gap";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VolumeGapRequest;
    type Response = VolumeGapResponse;

    fn check_scope(&self, request: &VolumeGapRequest, _: &CallerCtx) -> Result<(), EngineError> {
        if request.target.root.is_some() == request.target.volume_id.is_some() {
            return Err(EngineError::reason(OUT_OF_SCOPE));
        }
        Ok(())
    }

    fn execute(
        &self,
        request: VolumeGapRequest,
        caller: &CallerCtx,
    ) -> Result<VolumeGapResponse, EngineError> {
        let version = super::pin(&self.0, Some(&request.result_version), caller.channel)?;
        // Only a result of a confirmed root volume group reads the current mounts
        // (#263); every other result keeps its own scan-time volume report.
        let mounts = match (&self.1, version.volume_group()) {
            (Some(volumes), Some(_)) => volumes.enumerate().mounts.unwrap_or_default(),
            _ => Vec::new(),
        };
        let result = match (
            request.target.root.as_ref(),
            request.target.volume_id.as_ref(),
        ) {
            (Some(root), None) => {
                version.volume_gap_with_mounts(VolumeGapTarget::Root(root), &mounts)
            }
            (None, Some(volume)) => {
                let bytes = volume
                    .bytes()
                    .map_err(|_| EngineError::reason(OUT_OF_SCOPE))?;
                version.volume_gap_with_mounts(VolumeGapTarget::Volume(&bytes), &mounts)
            }
            _ => return Err(EngineError::reason(OUT_OF_SCOPE)),
        }
        .map_err(super::query_error)?;
        response(result, request.request_seq)
    }
}

fn response(result: PinnedVolumeGap, request_seq: u32) -> Result<VolumeGapResponse, EngineError> {
    let other_volumes = result.other_volumes;
    let factors: Vec<_> = result
        .gap
        .factors
        .into_iter()
        .map(|row| VolumeGapRow {
            key: row.key.into(),
            value: match row.value {
                FactorValue::Exact { bytes, evidence } => VolumeGapAmount::Exact {
                    bytes: bytes.into(),
                    evidence,
                },
                FactorValue::Unknown { reason_key } => VolumeGapAmount::Unknown { reason_key },
            },
        })
        .collect();
    let other_volumes =
        other_volumes.map(|rows| rows.into_iter().map(volume_row).collect::<Vec<_>>());
    if let Some(rows) = &other_volumes {
        validate_other_volume_total(rows, &factors)?;
    }
    Ok(VolumeGapResponse {
        result_version: result.result_version,
        request_seq,
        volume_id: result.volume_id.as_deref().map(PathBytes::from_bytes),
        start: observation(result.start),
        end: observation(result.end),
        allocated: VolumeGapSize {
            size: result.allocated,
            completeness: result.allocated_completeness,
        },
        unique_physical: VolumeGapSize {
            size: result.unique_physical,
            completeness: result.unique_physical_completeness,
        },
        factors,
        remainder: match result.gap.remainder {
            Ok(GapRemainder { bytes, evidence }) => VolumeGapAmount::Exact { bytes, evidence },
            Err(reason) => VolumeGapAmount::Unknown {
                reason_key: if reason == RemainderUnavailable::VolumeReportUnavailable {
                    UNAVAILABLE
                } else {
                    "space.data_insufficient"
                }
                .into(),
            },
        },
        os_categories: VolumeGapCategories::Unavailable {
            reason_key: UNAVAILABLE.into(),
        },
        other_volumes,
    })
}

fn volume_row(volume: OtherVolume) -> VolumeGapVolumeRow {
    VolumeGapVolumeRow {
        volume_name: volume.volume_name,
        role: Some(volume.role.into()),
        amount: match volume.amount {
            FactorValue::Exact { bytes, evidence } => VolumeGapAmount::Exact {
                bytes: bytes.into(),
                evidence,
            },
            FactorValue::Unknown { reason_key } => VolumeGapAmount::Unknown { reason_key },
        },
    }
}

fn validate_other_volume_total(
    rows: &[VolumeGapVolumeRow],
    factors: &[VolumeGapRow],
) -> Result<(), EngineError> {
    let out_of_scope = factors
        .iter()
        .find(|row| row.key == "gap.out_of_scope")
        .ok_or_else(|| EngineError::reason("space.data_insufficient"))?;
    let row_total = rows.iter().try_fold(0_u64, |sum, row| match &row.amount {
        VolumeGapAmount::Exact { bytes, .. } => sum.checked_add(bytes.get()),
        VolumeGapAmount::Unknown { .. } => None,
    });
    match (row_total, &out_of_scope.value) {
        (Some(total), VolumeGapAmount::Exact { bytes, .. }) if total == bytes.get() => Ok(()),
        (None, VolumeGapAmount::Unknown { .. }) => Ok(()),
        _ => Err(EngineError::reason("space.data_insufficient")),
    }
}

fn observation(point: Option<VolumeReportPoint>) -> VolumeGapObservation {
    let Some(point) = point else {
        return VolumeGapObservation::NotAttempted;
    };
    match point.value {
        Ok(volume) => VolumeGapObservation::Available {
            observed_at: point.observed_at,
            total_bytes: volume.total_bytes.into(),
            used_bytes: volume.used_bytes.into(),
            free_bytes: volume.free_bytes.into(),
            evidence: SizeEvidence::OsReported,
        },
        Err(error) => VolumeGapObservation::Unavailable {
            observed_at: point.observed_at,
            reason_key: error.reason_key,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact(bytes: u64) -> VolumeGapAmount {
        VolumeGapAmount::Exact {
            bytes: bytes.into(),
            evidence: SizeEvidence::OsReported,
        }
    }

    fn out_of_scope(value: VolumeGapAmount) -> Vec<VolumeGapRow> {
        vec![VolumeGapRow {
            key: "gap.out_of_scope".into(),
            value,
        }]
    }

    fn row(amount: VolumeGapAmount) -> VolumeGapVolumeRow {
        VolumeGapVolumeRow {
            volume_name: "VM".into(),
            role: Some(VolumeGapVolumeRow::ROLE_VM.into()),
            amount,
        }
    }

    #[test]
    fn validates_exact_and_unknown_other_volume_totals() {
        assert!(
            validate_other_volume_total(&[row(exact(4)), row(exact(6))], &out_of_scope(exact(10)))
                .is_ok()
        );
        let unknown = VolumeGapAmount::Unknown {
            reason_key: "space.data_insufficient".into(),
        };
        assert!(
            validate_other_volume_total(&[row(unknown.clone())], &out_of_scope(unknown.clone()))
                .is_ok()
        );
        assert!(validate_other_volume_total(&[row(exact(4))], &out_of_scope(exact(5))).is_err());
        assert!(validate_other_volume_total(&[row(unknown)], &out_of_scope(exact(0))).is_err());
    }

    #[test]
    fn volume_rows_keep_display_roles_and_os_evidence() {
        let converted = volume_row(OtherVolume {
            volume_name: "Preboot".into(),
            role: VolumeGapVolumeRow::ROLE_PREBOOT,
            amount: FactorValue::Exact {
                bytes: 12,
                evidence: SizeEvidence::OsReported,
            },
        });
        assert_eq!(converted.volume_name, "Preboot");
        assert_eq!(
            converted.role.as_deref(),
            Some(VolumeGapVolumeRow::ROLE_PREBOOT)
        );
        assert!(
            matches!(converted.amount, VolumeGapAmount::Exact { bytes, evidence: SizeEvidence::OsReported } if bytes.get() == 12)
        );
    }
}
