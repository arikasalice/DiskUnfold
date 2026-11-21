/// Summary of terminal history rows eligible for expiry at one reference time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryExpiryCandidates {
    /// Number of eligible history rows.
    pub count: u64,
    /// Earliest eligible reckon time, or `None` when no rows qualify.
    pub earliest_reckon_at_ms: Option<i64>,
    /// Latest eligible reckon time, or `None` when no rows qualify.
    pub latest_reckon_at_ms: Option<i64>,
    /// BLAKE3 digest of eligible 16-byte job IDs in ascending byte order.
    pub digest: [u8; 32],
}

/// Failure to read expiry candidates from persistent history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryExpiryReadError {
    /// The history storage could not provide the requested read.
    Unavailable,
}

/// Read-side port for bounded-memory history expiry candidate queries.
pub trait HistoryExpiryRead {
    /// Returns terminal histories expired by `days` at `reference_ms`.
    ///
    /// Implementations exclude histories without a terminal record or
    /// interruption marker, and must treat storage failures as unavailable.
    fn expiry_candidates(
        &mut self,
        days: u32,
        reference_ms: i64,
    ) -> Result<HistoryExpiryCandidates, HistoryExpiryReadError>;
}
