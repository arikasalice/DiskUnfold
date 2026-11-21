use crate::{Id, ports::PortError};

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

/// Immutable facts shared by one approval-check start or finish record.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Validated JSON summary; callers omit raw paths.
    pub destination_summary: Option<String>,
    /// Validated JSON summary; callers omit raw paths.
    pub keep_summary: Option<String>,
    /// The published DLD specifies five values but does not list their identifiers.
    pub check_result: String,
    /// JSON array of fields that did not match; empty when all fields matched.
    pub mismatch_fields: String,
    /// Time the request was approved, in milliseconds since the Unix epoch.
    pub approved_at_ms: i64,
    /// Time this check record was created, in milliseconds since the Unix epoch.
    pub created_at_ms: i64,
    /// Correlation identifier shared with the request and job history.
    pub correlation_id: Id,
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
    /// Final state of the compared operation.
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
    /// Present only for finish records.
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

    /// Reads rows for one request version in creation-time order.
    fn read_for_request(
        &self,
        request_version_id: &Id,
    ) -> Result<Vec<ApprovalCheckRecord>, PortError>;

    /// Reads rows for one job in creation-time order.
    fn read_for_job(&self, job_id: &Id) -> Result<Vec<ApprovalCheckRecord>, PortError>;
}
