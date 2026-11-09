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
        let code = match reason {
            "command.not_provided"
            | "tool.not_provided"
            | "command.envelope_invalid"
            | "command.body_invalid"
            | "command.channel_denied"
            | "cursor.invalid"
            | "cursor.limit_exceeded"
            | "path.unresolvable" => ErrorCode::InvalidArgument,
            "cursor.stale" | "cursor.expired" | "result.version_released" => {
                ErrorCode::StaleVersion
            }
            "path.outside_scope" | "path.link_escape" => ErrorCode::ScopeDenied,
            "db.busy" => ErrorCode::Busy,
            "db.schema_mismatch" | "support.internal_error" => ErrorCode::Internal,
            _ => return None,
        };
        let retryable = code == ErrorCode::Busy;
        Some(Self {
            code,
            reason_key: reason.to_owned(),
            message_key: reason.to_owned(),
            correlation_id,
            item_refs,
            retryable,
        })
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
