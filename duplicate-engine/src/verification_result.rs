//! Results and dispositions of duplicate content verification.

use crate::candidates::FileIdentity;
use crate::read_measurement::ReadMeasurement;
use std::collections::BTreeMap;

/// Item disposition. Candidates and deferred items are not verified duplicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Candidate,
    SampleCandidate,
    Distinct,
    PlaceholderDeferred,
    Unchecked,
    ReadFailed,
    Changed,
    Verified,
}
/// One independently identified physical file and its selected path references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedFile {
    pub item_nos: Vec<u32>,
    pub identity: FileIdentity,
    /// Whether the approved and observed state included a change stamp.
    pub change_stamp_available: bool,
}
/// A group formed only from full fingerprints produced by the pinned strategy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedGroup {
    pub size: u64,
    pub digest: Vec<u8>,
    pub files: Vec<VerifiedFile>,
}
/// Partial or complete in-memory result; cancellation leaves unfinished candidates unverified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub states: BTreeMap<u32, State>,
    pub groups: Vec<VerifiedGroup>,
    pub cancelled: bool,
    /// Candidate item references without a final disposition when cancelled.
    pub unjudged: Vec<u32>,
    /// Item references, including aliases, whose in-flight verification attempt
    /// returned after cancellation, including failed opens and observations.
    pub interrupted: Vec<u32>,
    /// Bytes returned by sample reads, including discarded in-flight work.
    pub sample_reads: ReadMeasurement,
    /// Bytes returned by full reads and EOF probes, including discarded work.
    pub full_reads: ReadMeasurement,
    pub algorithm_id: String,
    pub algorithm_revision: u32,
    pub sample_algorithm_id: String,
    pub sample_algorithm_revision: u32,
}

impl Outcome {
    pub(crate) fn mark_cancelled(&mut self) {
        self.cancelled = true;
        self.unjudged = self
            .states
            .iter()
            .filter_map(|(&item, state)| {
                matches!(state, State::Candidate | State::SampleCandidate).then_some(item)
            })
            .collect();
    }
}
