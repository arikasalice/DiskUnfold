//! F06 cancellation distinguishes a view subscription from its physical job.
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::Id;

/// Cancels exactly one of the targets defined by the visualization contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct VizCancelRequest {
    pub target: VizCancelTarget,
}

/// A subscription affects one view request. A job affects the shared computation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum VizCancelTarget {
    Subscription {
        view_channel: Id,
        request_seq: NonZeroU32,
    },
    Job {
        job_ref: Id,
    },
}

/// Acknowledges control admission without claiming physical job completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VizCancelResponse {
    pub control_result: VizControlResult,
}

/// The linearized outcome of a visualization cancellation request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum VizControlResult {
    Accepted,
    NoEffect,
    AlreadyTerminal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn public_visualization_errors_keep_their_registered_codes() {
        use crate::{ErrorCode, ErrorEnvelope};
        let correlation = Id::from_v7_bytes([7; 16]);
        for (reason, code, retryable) in [
            ("viz.busy", ErrorCode::Busy, true),
            ("viz.viewport_invalid", ErrorCode::InvalidArgument, false),
        ] {
            let error = ErrorEnvelope::from_reason_or_internal(reason, correlation.clone(), vec![]);
            assert_eq!(error.reason_key, reason);
            assert_eq!(error.code, code);
            assert_eq!(error.retryable, retryable);
            assert_eq!(error.correlation_id, correlation);
        }
    }

    #[test]
    fn cancellation_targets_round_trip_without_mixing_their_fields() -> Result<(), serde_json::Error>
    {
        let reference = Id::from_v7_bytes([5; 16]);
        for target in [
            json!({"kind":"subscription", "viewChannel":reference, "requestSeq":1}),
            json!({"kind":"subscription", "viewChannel":reference, "requestSeq":u32::MAX}),
            json!({"kind":"job", "jobRef":reference}),
        ] {
            let request = json!({"target":target});
            let parsed: VizCancelRequest = serde_json::from_value(request.clone())?;
            assert_eq!(serde_json::to_value(parsed)?, request);
        }
        for control in ["accepted", "no_effect", "already_terminal"] {
            let response = json!({"controlResult":control});
            let parsed: VizCancelResponse = serde_json::from_value(response.clone())?;
            assert_eq!(serde_json::to_value(parsed)?, response);
        }
        Ok(())
    }

    #[test]
    fn rejects_invalid_sequences_missing_targets_and_mixed_fields() {
        let reference = Id::from_v7_bytes([6; 16]);
        let valid = json!({"kind":"job", "jobRef":reference});
        let invalid = [
            json!({}),
            json!({"target":null}),
            json!({"target":valid, "jobRef":reference}),
            json!({"target":{"kind":"job", "jobRef":reference, "requestSeq":1}}),
            json!({"target":{"kind":"subscription", "viewChannel":reference}}),
            json!({"target":{"kind":"subscription", "viewChannel":reference, "requestSeq":0}}),
            json!({"target":{"kind":"subscription", "viewChannel":reference, "requestSeq":4_294_967_296u64}}),
            json!({"target":{"kind":"subscription", "viewChannel":reference, "requestSeq":-1}}),
            json!({"target":{"kind":"subscription", "viewChannel":reference, "requestSeq":1, "jobRef":reference}}),
        ];
        for value in invalid {
            assert!(serde_json::from_value::<VizCancelRequest>(value).is_err());
        }
    }
}
