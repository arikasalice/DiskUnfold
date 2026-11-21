use crate::Id;

/// One expired terminal history and its follow-up eligibility.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryExpiryCandidate {
    /// The persistent 16-byte job reference.
    pub job_id: Id,
    /// Whether a successor that names this job as predecessor is nonterminal.
    pub has_nonterminal_follow_up: bool,
    /// The candidate's history reckon time.
    pub reckon_at_ms: i64,
}

/// Summary of terminal history rows eligible for expiry at one reference time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryExpiryCandidates {
    /// The history deletion generation from the same read snapshot.
    pub delete_generation: u64,
    /// Number of eligible history rows.
    pub count: u64,
    /// Earliest eligible reckon time, or `None` when no rows qualify.
    pub earliest_reckon_at_ms: Option<i64>,
    /// Latest eligible reckon time, or `None` when no rows qualify.
    pub latest_reckon_at_ms: Option<i64>,
    /// BLAKE3 digest of eligible 16-byte job IDs in ascending byte order.
    pub digest: [u8; 32],
    /// Eligible rows in ascending job-ID byte order.
    pub candidates: Vec<HistoryExpiryCandidate>,
}

/// Failure to read expiry candidates from persistent history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryExpiryReadError {
    /// The history storage could not provide the requested read.
    Unavailable,
}

/// Read-side port for history expiry candidate queries.
pub trait HistoryExpiryRead: Send {
    /// Reads the deletion generation without scanning history candidates.
    fn delete_generation(&mut self) -> Result<u64, HistoryExpiryReadError>;

    /// Returns terminal histories expired by `days` at `reference_ms`.
    ///
    /// Implementations exclude histories without a terminal record or
    /// interruption marker, and must treat storage failures as unavailable.
    /// Candidate IDs and the digest are ordered by the raw ID bytes. The
    /// deletion generation, rows, range and digest come from one read snapshot.
    fn expiry_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
    ) -> Result<HistoryExpiryCandidates, HistoryExpiryReadError>;
}
