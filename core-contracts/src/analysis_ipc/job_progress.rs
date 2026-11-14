//! The F24 `job.progress` event body (HLD IF-06).
use super::AnalysisJobWaitReason;
use crate::{Timestamp, job::ProgressUnit};
use serde::{Deserialize, Serialize};

/// Topic of the batched job progress event.
pub const JOB_PROGRESS: &str = "job.progress";

/// The latest progress of one job, sent at most once per batch interval.
///
/// The envelope carries the job reference and, when one is available, the
/// latest partial result version for reference. Intermediate values may be
/// omitted. Receivers discard a value whose sequence is not greater than the
/// last sequence applied for the job, and any value after the terminal event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJobProgress {
    /// Per-job sequence shared with state changes and the terminal event.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    /// The progress unit declared by the job kind.
    pub unit: ProgressUnit,
    /// Units processed so far.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub completed: u64,
    /// The confirmed total, omitted while the total is not established.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "number"))]
    pub total: Option<u64>,
    /// Whether `total` is confirmed.
    pub total_known: bool,
    /// The wait reason observed with this value.
    pub wait_reason: AnalysisJobWaitReason,
    /// UTC time at which the runtime observed this value.
    pub observed_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_total_is_omitted_and_known_total_is_numeric()
    -> Result<(), Box<dyn std::error::Error>> {
        let observed_at = Timestamp::from_unix_millis(0).ok_or("time")?;
        let mut body = AnalysisJobProgress {
            sequence: 3,
            unit: ProgressUnit::Items,
            completed: 7,
            total: None,
            total_known: false,
            wait_reason: AnalysisJobWaitReason::Unknown,
            observed_at,
        };
        assert_eq!(
            serde_json::to_value(&body)?,
            serde_json::json!({
                "sequence": 3,
                "unit": "items",
                "completed": 7,
                "totalKnown": false,
                "waitReason": "unknown",
                "observedAt": "1970-01-01T00:00:00.000Z",
            })
        );
        body.total = Some(10);
        body.total_known = true;
        body.unit = ProgressUnit::Bytes;
        let wire = serde_json::to_value(&body)?;
        assert_eq!(wire["total"], 10);
        assert_eq!(wire["unit"], "bytes");
        assert_eq!(serde_json::from_value::<AnalysisJobProgress>(wire)?, body);
        Ok(())
    }
}
