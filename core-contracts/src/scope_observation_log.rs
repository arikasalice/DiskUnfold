//! Append-only scan-completion observations and the ports that store and read them.
//!
//! One observation is one line of totals for a scope at the time a GUI scan finished.
//! Analysis-index decides what the values mean, app-db stores them, and these ports
//! keep the two crates independent of each other.

use crate::{Id, SizeEvidence, result::ResultCompleteness};

/// Fingerprint format of the 32-byte BLAKE3 scope digest shared with recent scopes.
pub const SCOPE_FINGERPRINT_FORMAT_V1: u32 = 1;

/// The most rows one [`ScopeObservationRead::list`] call returns.
pub const SCOPE_OBSERVATION_LIST_LIMIT: usize = 1_000;

/// The kind of result version an observation was taken from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeObservationOrigin {
    /// A full scan.
    Scan,
    /// An incremental replacement of an earlier scan.
    Incremental,
}

/// What is known about one total.
///
/// Unknown carries no byte count, so an unknown total can never be mistaken for zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservedAmount {
    /// The exact number of bytes.
    Exact(u64),
    /// At least this many bytes.
    LowerBound(u64),
    /// No usable value.
    Unknown,
}

/// One total with the evidence the sealed result version gave for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedSize {
    /// Exact, lower-bound or unknown amount.
    pub amount: ObservedAmount,
    /// Where the amount came from.
    pub evidence: SizeEvidence,
}

/// The totals of one scope at the end of one scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeObservation {
    /// UUID v7 identifier of this row.
    pub observation_id: Id,
    /// 32-byte scope fingerprint, in the format named by `fingerprint_format`.
    pub scope_fingerprint: [u8; 32],
    /// Fingerprint format; see [`SCOPE_FINGERPRINT_FORMAT_V1`].
    pub fingerprint_format: u32,
    /// The collector's observation time in Unix milliseconds.
    pub observed_at_ms: i64,
    /// The scan job that produced the sealed result. At most one row exists per job.
    pub job_id: Id,
    /// The kind of result the totals were read from.
    pub origin: ScopeObservationOrigin,
    /// Completeness of the sealed result.
    pub completeness: ResultCompleteness,
    /// Identifier of the size aggregation rule that produced the totals.
    pub size_rule_id: String,
    /// Revision of that rule; at least one.
    pub size_rule_revision: u32,
    /// Logical size of the scope.
    pub logical: ObservedSize,
    /// Allocated size of the scope.
    pub allocated: ObservedSize,
    /// Unique physical size of the scope.
    pub unique_physical: ObservedSize,
    /// Time the row was made, in Unix milliseconds. A queued row can be stored later.
    pub created_at_ms: i64,
}

/// Result of a successful append.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeObservationAppend {
    /// The row was stored.
    Added,
    /// A row for the same job already existed and was left unchanged.
    AlreadyRecorded,
}

/// Why an append did not store its row. Nothing was written in any case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeObservationWriteError {
    /// A value cannot be stored, for example a byte count of 2^63 or more.
    Invalid,
    /// The database stayed locked for the connection's busy timeout. The attempt rolled
    /// back; the caller may try the same row again.
    Busy,
    /// The filesystem is full.
    DiskFull,
    /// Another storage operation failed.
    Storage,
}

/// Appends observations. Each call is one transaction.
pub trait ScopeObservationLog: Send + Sync {
    /// Stores one observation and applies the retention limits in the same transaction.
    ///
    /// A second call for the same job returns [`ScopeObservationAppend::AlreadyRecorded`].
    /// The caller must not treat a failure as a failure of the scan.
    fn append(
        &self,
        observation: &ScopeObservation,
    ) -> Result<ScopeObservationAppend, ScopeObservationWriteError>;
}

/// Rows of one scope in observation order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeObservationList {
    /// Rows by observation time, then observation identifier, both ascending.
    pub rows: Vec<ScopeObservation>,
    /// True when more rows matched than `limit` allowed. The earliest rows are returned.
    pub truncated: bool,
}

/// Why a list could not be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeObservationReadError {
    /// The database predates the observation table. This is not an empty history.
    UnsupportedSchema,
    /// The limit is zero or above [`SCOPE_OBSERVATION_LIST_LIMIT`], or the fingerprint
    /// format is below one.
    InvalidQuery,
    /// The database stayed locked.
    Busy,
    /// A stored row violates the table contract.
    Corrupt,
    /// Another storage operation failed.
    Storage,
}

/// Reads observations. Thinning and interval grouping belong to the caller.
pub trait ScopeObservationRead {
    /// Lists one scope's rows with `from_ms <= observed_at_ms <= to_ms`; a missing bound is open.
    ///
    /// Returns at most `limit` rows, the earliest first. `limit` is between one and
    /// [`SCOPE_OBSERVATION_LIST_LIMIT`]. An empty list means no matching observation.
    fn list(
        &self,
        scope_fingerprint: &[u8; 32],
        fingerprint_format: u32,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: usize,
    ) -> Result<ScopeObservationList, ScopeObservationReadError>;
}
