use crate::{Id, ports::PortError};
use std::fmt;

/// Request source recorded for an approval check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalRequestKind {
    /// A cleanup plan.
    Plan,
    /// A restore request.
    Restore,
    /// An undo request for a same-volume move.
    MoveUndo,
}

/// Operation recorded for an approval check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalOperationKind {
    /// Move an item to the operating-system trash.
    Trash,
    /// Move an item to another location.
    Move,
    /// Permanently delete an item.
    Delete,
    /// Restore a previously moved item.
    Restore,
    /// Undo a same-volume move.
    MoveUndo,
}

/// Result of the one-time approval comparison immediately before execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalCheckResult {
    /// Every approval fact matched and the approval was consumed.
    Valid,
    /// No issued approval was found.
    NotIssued,
    /// The approval was already consumed or is currently in use.
    AlreadyUsed,
    /// One or more compared facts did not match.
    Mismatch,
    /// The required confirmation proof was missing.
    ConfirmationMissing,
}

impl ApprovalCheckResult {
    /// Returns the stable value stored in the database.
    pub const fn as_db_value(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::NotIssued => "not_issued",
            Self::AlreadyUsed => "already_used",
            Self::Mismatch => "mismatch",
            Self::ConfirmationMissing => "confirmation_missing",
        }
    }

    /// Parses one stable database value.
    pub fn from_db_value(value: &str) -> Option<Self> {
        match value {
            "valid" => Some(Self::Valid),
            "not_issued" => Some(Self::NotIssued),
            "already_used" => Some(Self::AlreadyUsed),
            "mismatch" => Some(Self::Mismatch),
            "confirmation_missing" => Some(Self::ConfirmationMissing),
            _ => None,
        }
    }
}

/// A path-free comparison key that may appear in `mismatch_fields`.
///
/// The F28 DLD identifies the compared facts but leaves their serialized keys
/// open; these stable keys name the secret, session, plan, operation, and
/// destination/keep comparisons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalMismatchField {
    /// Approval secret comparison failed.
    Secret,
    /// Application session comparison failed.
    AppSessionId,
    /// Request plan digest comparison failed.
    PlanHash,
    /// Requested operation comparison failed.
    OperationKind,
    /// Destination summary comparison failed.
    DestinationSummary,
    /// Keep-condition summary comparison failed.
    KeepSummary,
}

impl ApprovalMismatchField {
    /// Returns the stable, path-free comparison key stored in JSON.
    pub const fn as_json_value(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::AppSessionId => "app_session_id",
            Self::PlanHash => "plan_hash",
            Self::OperationKind => "operation_kind",
            Self::DestinationSummary => "destination_summary",
            Self::KeepSummary => "keep_summary",
        }
    }

    /// Parses one known, path-free comparison key.
    pub fn from_json_value(value: &str) -> Option<Self> {
        match value {
            "secret" => Some(Self::Secret),
            "app_session_id" => Some(Self::AppSessionId),
            "plan_hash" => Some(Self::PlanHash),
            "operation_kind" => Some(Self::OperationKind),
            "destination_summary" => Some(Self::DestinationSummary),
            "keep_summary" => Some(Self::KeepSummary),
            _ => None,
        }
    }
}

/// Immutable facts shared by one approval-check start or finish record.
#[derive(Clone, PartialEq, Eq)]
pub struct ApprovalCheckFacts {
    /// UUID v7 identifier for this immutable check row.
    pub check_id: Id,
    /// Job associated with this check, when one has been assigned.
    pub job_id: Option<Id>,
    /// Identifier of the plan or other request version being checked.
    pub request_version_id: Id,
    /// Kind of request that supplied the version.
    pub request_kind: ApprovalRequestKind,
    /// Stored request-version digest.
    pub version_hash: [u8; 32],
    /// Tag identifying the digest format.
    pub hash_format: String,
    /// File operation covered by the approval check.
    pub operation_kind: ApprovalOperationKind,
    /// Validated JSON summary up to 4 KiB; callers omit raw paths.
    pub destination_summary: Option<String>,
    /// Validated JSON summary up to 4 KiB; callers omit raw paths.
    pub keep_summary: Option<String>,
    /// Result of the comparison, stored as one of the five database values.
    pub check_result: ApprovalCheckResult,
    /// JSON array of known comparison keys that did not match; contains no values or paths.
    pub mismatch_fields: String,
    /// Time the request was approved, in milliseconds since the Unix epoch.
    pub approved_at_ms: i64,
    /// Time this check record was created, in milliseconds since the Unix epoch.
    pub created_at_ms: i64,
    /// Correlation identifier shared with the request and job history.
    pub correlation_id: Id,
}

impl fmt::Debug for ApprovalCheckFacts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApprovalCheckFacts")
            .field("check_id", &self.check_id)
            .field("job_id", &self.job_id)
            .field("request_version_id", &self.request_version_id)
            .field("request_kind", &self.request_kind)
            .field("operation_kind", &self.operation_kind)
            .field("check_result", &self.check_result)
            .field(
                "destination_summary_len",
                &self.destination_summary.as_ref().map(String::len),
            )
            .field(
                "keep_summary_len",
                &self.keep_summary.as_ref().map(String::len),
            )
            .field("mismatch_fields_len", &self.mismatch_fields.len())
            .field("approved_at_ms", &self.approved_at_ms)
            .field("created_at_ms", &self.created_at_ms)
            .field("correlation_id", &self.correlation_id)
            .finish()
    }
}

/// Input for one start record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalCheckStartRecord {
    /// Facts captured when the comparison begins.
    pub facts: ApprovalCheckFacts,
}

/// Input for one finish record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalCheckFinishRecord {
    /// Facts captured when the comparison finishes.
    pub facts: ApprovalCheckFacts,
    /// Non-empty final state. The F28 DLD leaves its allowed value set open.
    pub end_state: String,
}

/// Kind of an approval-check history row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalCheckRecordKind {
    /// Start of one comparison.
    Start,
    /// Finish of one comparison.
    Finish,
}

/// One stored approval-check history row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalCheckRecord {
    /// Immutable comparison facts persisted with this row.
    pub facts: ApprovalCheckFacts,
    /// Whether this row records the beginning or end of a comparison.
    pub record_kind: ApprovalCheckRecordKind,
    /// Present only for finish records; the F28 DLD leaves its allowed value set open.
    pub end_state: Option<String>,
}

/// Appends start and finish rows and reads the two documented history views.
///
/// Each append is one independent transaction. SQLite writes wait up to 5,000 ms
/// and return `db.busy` without retrying. Read results are for history display
/// only and must never authorize a file change.
pub trait ApprovalCheckRecorder {
    /// Appends one start row without retrying a failed write.
    fn append_start(&mut self, record: &ApprovalCheckStartRecord) -> Result<(), PortError>;

    /// Appends one finish row without retrying a failed write.
    fn append_finish(&mut self, record: &ApprovalCheckFinishRecord) -> Result<(), PortError>;

    /// Reads rows for one request version in creation-time and check-ID order.
    fn read_for_request(
        &self,
        request_version_id: &Id,
    ) -> Result<Vec<ApprovalCheckRecord>, PortError>;

    /// Reads rows for one job in creation-time and check-ID order.
    fn read_for_job(&self, job_id: &Id) -> Result<Vec<ApprovalCheckRecord>, PortError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(seed: u8) -> Id {
        let mut bytes = [0; 16];
        bytes[0] = seed;
        Id::from_v7_bytes(bytes)
    }

    #[test]
    fn check_result_values_round_trip_one_to_one() {
        for (result, value) in [
            (ApprovalCheckResult::Valid, "valid"),
            (ApprovalCheckResult::NotIssued, "not_issued"),
            (ApprovalCheckResult::AlreadyUsed, "already_used"),
            (ApprovalCheckResult::Mismatch, "mismatch"),
            (
                ApprovalCheckResult::ConfirmationMissing,
                "confirmation_missing",
            ),
        ] {
            assert_eq!(result.as_db_value(), value);
            assert_eq!(ApprovalCheckResult::from_db_value(value), Some(result));
        }
        assert_eq!(ApprovalCheckResult::from_db_value("unknown"), None);
    }

    #[test]
    fn debug_redacts_summary_paths() {
        let destination_path = r#"C:\Users\private\destination"#;
        let keep_path = r#"D:\Users\private\keep.txt"#;
        let facts = ApprovalCheckFacts {
            check_id: id(1),
            job_id: Some(id(2)),
            request_version_id: id(3),
            request_kind: ApprovalRequestKind::Plan,
            version_hash: [4; 32],
            hash_format: "du.plan.hash.v1".into(),
            operation_kind: ApprovalOperationKind::Delete,
            destination_summary: Some(format!(r#"{{"path":"{destination_path}"}}"#)),
            keep_summary: Some(format!(r#"{{"path":"{keep_path}"}}"#)),
            check_result: ApprovalCheckResult::Valid,
            mismatch_fields: "[]".into(),
            approved_at_ms: 10,
            created_at_ms: 11,
            correlation_id: id(5),
        };

        let debug = format!("{facts:?}");
        assert!(debug.contains(&format!(
            "destination_summary_len: Some({})",
            facts.destination_summary.as_ref().map(String::len).unwrap_or_default()
        )));
        assert!(debug.contains(&format!(
            "keep_summary_len: Some({})",
            facts.keep_summary.as_ref().map(String::len).unwrap_or_default()
        )));
        assert!(!debug.contains(destination_path));
        assert!(!debug.contains(keep_path));
    }
}
