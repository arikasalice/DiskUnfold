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
            | "locale.unsupported"
            | "locale.format_unknown"
            | "settings.value_invalid"
            | "cursor.invalid"
            | "cursor.limit_exceeded"
            | "cursor.mismatch"
            | "browse.not_folder"
            | "query.syntax_invalid"
            | "condition.ambiguous"
            | "condition.timezone_unavailable"
            | "predicate.unknown"
            | "predicate.not_allowed"
            | "sort.key_unknown"
            | "viz.viewport_invalid"
            | "viz.format_unknown"
            | "snapshot.format_unknown"
            | "viz.strategy_unknown"
            | "viz.measure_unknown"
            | "viz.denominator_invalid"
            | "space.target_out_of_scope"
            | "scope.empty"
            | "scope.target_unavailable"
            | "scope.exclude_invalid"
            | "scan.priority_out_of_scope"
            | "path.unresolvable"
            | "sample.mixed_source"
            | "snapshot.sample_source"
            | "startup.shutting_down" => ErrorCode::InvalidArgument,
            "cursor.stale"
            | "cursor.expired"
            | "result.version_released"
            | "result.version_stale"
            | "scope.changed"
            | "result.requery_required"
            | "history.version_changed" => ErrorCode::StaleVersion,
            "locale.profile_modified" => ErrorCode::StaleVersion,
            "ref.unavailable"
            | "result.version_not_found"
            | "browse.location_missing"
            | "job.not_found"
            | "history.removed"
            | "viz.model_expired"
            | "sample.session_closed"
            | "locale.bundle_unavailable" => ErrorCode::NotFound,
            "command.window_denied" => ErrorCode::OriginDenied,
            "path.outside_scope" | "path.link_escape" => ErrorCode::ScopeDenied,
            "db.busy" | "job.queue_full" | "job.shutting_down" | "query.busy" | "viz.busy" => {
                ErrorCode::Busy
            }
            "db.schema_mismatch"
            | "support.internal_error"
            | "history.storage_unavailable"
            | "locale.registration_failed" => ErrorCode::Internal,
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

#[cfg(test)]
mod tests {
    use super::{ErrorCode, ErrorEnvelope};
    use crate::Id;

    #[test]
    fn locale_profile_reasons_keep_the_published_error_codes()
    -> Result<(), Box<dyn std::error::Error>> {
        let correlation = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
        for (reason, expected) in [
            ("locale.unsupported", ErrorCode::InvalidArgument),
            ("locale.format_unknown", ErrorCode::InvalidArgument),
            (
                crate::snapshot_format::SNAPSHOT_FORMAT_UNKNOWN,
                ErrorCode::InvalidArgument,
            ),
            ("settings.value_invalid", ErrorCode::InvalidArgument),
            ("locale.profile_modified", ErrorCode::StaleVersion),
            ("locale.bundle_unavailable", ErrorCode::NotFound),
            ("command.window_denied", ErrorCode::OriginDenied),
            ("locale.registration_failed", ErrorCode::Internal),
            ("db.busy", ErrorCode::Busy),
        ] {
            let envelope = ErrorEnvelope::from_reason(reason, correlation.clone(), Vec::new())
                .ok_or("locale reason was not registered")?;
            assert_eq!(envelope.reason_key, reason);
            assert_eq!(envelope.code, expected);
        }
        Ok(())
    }

    #[test]
    fn permanent_closing_is_not_retryable_but_transient_job_shutdown_is()
    -> Result<(), Box<dyn std::error::Error>> {
        let correlation = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
        let closing =
            ErrorEnvelope::from_reason("startup.shutting_down", correlation.clone(), Vec::new())
                .ok_or("startup.shutting_down is not registered")?;
        assert_eq!(closing.code, ErrorCode::InvalidArgument);
        assert!(
            !closing.retryable,
            "the app is leaving; the command can never succeed"
        );
        let job = ErrorEnvelope::from_reason("job.shutting_down", correlation, Vec::new())
            .ok_or("job.shutting_down is not registered")?;
        assert_eq!(job.code, ErrorCode::Busy);
        Ok(())
    }

    #[test]
    fn sample_reasons_keep_the_published_error_codes() -> Result<(), Box<dyn std::error::Error>> {
        let correlation = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
        for (reason, expected) in [
            (
                crate::data_source::SAMPLE_MIXED_SOURCE,
                ErrorCode::InvalidArgument,
            ),
            (
                crate::sample_ipc::SNAPSHOT_SAMPLE_SOURCE,
                ErrorCode::InvalidArgument,
            ),
            (
                crate::sample_ipc::SAMPLE_SESSION_CLOSED,
                ErrorCode::NotFound,
            ),
        ] {
            let envelope = ErrorEnvelope::from_reason(reason, correlation.clone(), Vec::new())
                .ok_or("sample reason was not registered")?;
            assert_eq!(envelope.code, expected);
            assert!(!envelope.retryable);
        }
        Ok(())
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
