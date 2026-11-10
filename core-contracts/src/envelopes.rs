use crate::{Id, ItemRef, Timestamp};
use serde::{Deserialize, Serialize};

pub const ENVELOPE_VERSION: u32 = 1;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    AuthRequired,
    OriginDenied,
    ScopeDenied,
    InvalidArgument,
    NotFound,
    StaleVersion,
    Busy,
    #[serde(other)]
    Internal,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ErrorEnvelope {
    pub code: ErrorCode,
    pub reason_key: String,
    pub message_key: String,
    pub correlation_id: Id,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub item_refs: Vec<ItemRef>,
    pub retryable: bool,
}
impl ErrorEnvelope {
    pub fn from_reason(reason: &str, correlation_id: Id, item_refs: Vec<ItemRef>) -> Option<Self> {
        Some(Self::new(
            reason,
            Self::code_for(reason)?,
            correlation_id,
            item_refs,
        ))
    }

    pub fn from_reason_or_internal(
        reason: &str,
        correlation_id: Id,
        item_refs: Vec<ItemRef>,
    ) -> Self {
        match Self::code_for(reason) {
            Some(code) => Self::new(reason, code, correlation_id, item_refs),
            None => Self::new(
                "support.internal_error",
                ErrorCode::Internal,
                correlation_id,
                Vec::new(),
            ),
        }
    }

    fn code_for(reason: &str) -> Option<ErrorCode> {
        Some(match reason {
            "command.not_provided"
            | "tool.not_provided"
            | "command.envelope_invalid"
            | "command.body_invalid"
            | "command.channel_denied"
            | "cursor.invalid"
            | "cursor.limit_exceeded"
            | "space.target_out_of_scope"
            | "scope.empty"
            | "scope.target_unavailable"
            | "scope.exclude_invalid"
            | "path.unresolvable" => ErrorCode::InvalidArgument,
            "cursor.stale"
            | "cursor.expired"
            | "result.version_released"
            | "scope.changed"
            | "result.requery_required"
            | "history.version_changed" => ErrorCode::StaleVersion,
            "ref.unavailable" | "job.not_found" => ErrorCode::NotFound,
            "path.outside_scope" | "path.link_escape" => ErrorCode::ScopeDenied,
            "db.busy" | "job.queue_full" | "job.shutting_down" => ErrorCode::Busy,
            "db.schema_mismatch" | "support.internal_error" | "history.storage_unavailable" => {
                ErrorCode::Internal
            }
            _ => return None,
        })
    }

    fn new(reason: &str, code: ErrorCode, correlation_id: Id, item_refs: Vec<ItemRef>) -> Self {
        let retryable = code == ErrorCode::Busy;
        Self {
            code,
            reason_key: reason.to_owned(),
            message_key: reason.to_owned(),
            correlation_id,
            item_refs,
            retryable,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CallEnvelope {
    pub name: String,
    pub contract_version: u32,
    pub correlation_id: Id,
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    pub body: serde_json::Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResultEnvelope {
    pub name: String,
    pub contract_version: u32,
    pub correlation_id: Id,
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    pub body: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub error: Option<ErrorEnvelope>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub topic: String,
    pub contract_version: u32,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub correlation_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub job_ref: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub result_version: Option<Id>,
    pub occurred_at: Timestamp,
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    pub body: serde_json::Value,
}
impl EventEnvelope {
    pub fn validate(&self) -> bool {
        self.contract_version == ENVELOPE_VERSION && self.seq > 0 && self.seq < (1_u64 << 53)
    }
}
