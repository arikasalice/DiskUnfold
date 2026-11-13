//! Admits a read-only volume explanation against the current pinned result.
use du_analysis_index::{
    IngestSink, VolumeReportPoint,
    volume_gap::{
        FactorValue, GapRemainder, PinnedVolumeGap, RemainderUnavailable, VolumeGapTarget,
    },
};
use du_core_contracts::{
    PathBytes, SizeEvidence,
    volume_gap_ipc::{
        VolumeGapAmount, VolumeGapCategories, VolumeGapObservation, VolumeGapRequest,
        VolumeGapResponse, VolumeGapRow, VolumeGapSize,
    },
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

const UNAVAILABLE: &str = "space.volume_report_unavailable";
const OUT_OF_SCOPE: &str = "space.target_out_of_scope";

pub(crate) fn register(builder: &mut EngineBuilder, store: IngestSink) {
    builder.register(Query(store));
}

struct Query(IngestSink);

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
        _: &CallerCtx,
    ) -> Result<VolumeGapResponse, EngineError> {
        let version = super::pin(&self.0, Some(&request.result_version))?;
        let result = match (
            request.target.root.as_ref(),
            request.target.volume_id.as_ref(),
        ) {
            (Some(root), None) => version.volume_gap(VolumeGapTarget::Root(root)),
            (None, Some(volume)) => {
                let bytes = volume
                    .bytes()
                    .map_err(|_| EngineError::reason(OUT_OF_SCOPE))?;
                version.volume_gap(VolumeGapTarget::Volume(&bytes))
            }
            _ => return Err(EngineError::reason(OUT_OF_SCOPE)),
        }
        .map_err(super::query_error)?;
        Ok(response(result, request.request_seq))
    }
}

fn response(result: PinnedVolumeGap, request_seq: u32) -> VolumeGapResponse {
    VolumeGapResponse {
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
        factors: result
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
            .collect(),
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
