//! Desktop admission contract for prioritizing a folder in a live scan.
use crate::{Id, ItemRef, analysis_ipc::JobControlOutcome};
use serde::{Deserialize, Deserializer, Serialize};

/// Requests normal scheduling priority for an observed folder in one owned scan.
///
/// `priority_area` must come from the same scan's current partial result. The
/// engine resolves its raw root-relative evidence before reaching scan-engine;
/// callers must not send a display path or a root index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanPriorityRequest {
    /// Job reference returned when the scan was admitted.
    pub job_ref: Id,
    /// Existing item reference selected from the scan's current partial result.
    pub priority_area: ItemRef,
}

impl<'de> Deserialize<'de> for ScanPriorityRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Request {
            job_ref: Id,
            priority_area: StrictItemRef,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct StrictItemRef {
            result_version: Id,
            item_no: u32,
        }

        let request = Request::deserialize(deserializer)?;
        Ok(Self {
            job_ref: request.job_ref,
            priority_area: ItemRef {
                result_version: request.priority_area.result_version,
                item_no: request.priority_area.item_no,
            },
        })
    }
}

/// Acknowledges priority admission without claiming that work has completed.
///
/// Responses accept unknown fields so that a later contract revision can add
/// optional values without breaking an older reader.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanPriorityResponse {
    /// The existing job-control result vocabulary.
    pub control_result: JobControlOutcome,
    /// Present when the request remains the applied priority area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub applied_priority_area: Option<ItemRef>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_and_response_use_the_existing_item_and_control_shapes()
    -> Result<(), serde_json::Error> {
        let reference = Id::from_v7_bytes([1; 16]);
        let request = ScanPriorityRequest {
            job_ref: reference.clone(),
            priority_area: ItemRef {
                result_version: reference.clone(),
                item_no: 7,
            },
        };
        let value = json!({
            "jobRef": reference.clone(),
            "priorityArea": {"resultVersion": reference.clone(), "itemNo": 7}
        });
        assert_eq!(serde_json::to_value(&request)?, value);
        assert_eq!(
            serde_json::from_value::<ScanPriorityRequest>(value.clone())?,
            request
        );

        let response = ScanPriorityResponse {
            control_result: JobControlOutcome::Accepted,
            applied_priority_area: Some(request.priority_area.clone()),
        };
        assert_eq!(
            serde_json::to_value(response)?,
            json!({
                "controlResult": "accepted",
                "appliedPriorityArea": {
                    "resultVersion": reference.clone(),
                    "itemNo": 7
                }
            })
        );
        assert_eq!(
            serde_json::to_value(ScanPriorityResponse {
                control_result: JobControlOutcome::AlreadyTerminal,
                applied_priority_area: None,
            })?,
            json!({"controlResult": "already_terminal"})
        );
        Ok(())
    }

    #[test]
    fn priority_request_rejects_unknown_body_fields() {
        let value = json!({
            "jobRef": "018f0000-0000-7000-8000-000000000001",
            "priorityArea": {
                "resultVersion": "018f0000-0000-7000-8000-000000000002",
                "itemNo": 7
            },
            "path": "C:/untrusted"
        });
        assert!(serde_json::from_value::<ScanPriorityRequest>(value).is_err());

        let value = json!({
            "jobRef": "018f0000-0000-7000-8000-000000000001",
            "priorityArea": {
                "resultVersion": "018f0000-0000-7000-8000-000000000002",
                "itemNo": 7,
                "displayPath": "C:/untrusted"
            }
        });
        assert!(serde_json::from_value::<ScanPriorityRequest>(value).is_err());
    }

    #[test]
    fn priority_response_accepts_unknown_fields() -> Result<(), serde_json::Error> {
        let response = serde_json::from_value::<ScanPriorityResponse>(json!({
            "controlResult": "no_effect",
            "addedLater": true
        }))?;
        assert_eq!(response.control_result, JobControlOutcome::NoEffect);
        assert_eq!(response.applied_priority_area, None);
        Ok(())
    }

    #[test]
    fn out_of_scope_reason_is_registered_as_invalid_argument()
    -> Result<(), Box<dyn std::error::Error>> {
        let correlation = Id::parse("018f0000-0000-7000-8000-000000000003")?;
        let error = crate::ErrorEnvelope::from_reason(
            "scan.priority_out_of_scope",
            correlation,
            Vec::new(),
        )
        .ok_or("priority reason is not registered")?;
        assert_eq!(error.code, crate::ErrorCode::InvalidArgument);
        assert!(!error.retryable);
        Ok(())
    }
}
