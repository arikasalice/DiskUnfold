//! F19 session-only catalog link-status decisions from already classified evidence.
//!
//! The caller must obtain storage correspondence from F08's published matcher.
//! This module does not enumerate volumes, validate snapshots, or prove that an
//! observed file currently exists. A fresh session starts at [`Status::Unchecked`].

/// The published link-status decision table identifier.
pub const RULE_ID: &str = "catalog.link-status";
/// The published decision table revision.
pub const RULE_REVISION: u32 = 1;

/// Completeness of a successful current-volume enumeration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enumeration {
    /// All volumes supported by the port were enumerated.
    Complete,
    /// The port may have omitted volumes.
    Partial,
    /// Enumeration failed, is unsupported, or is unknown.
    Unavailable,
}

/// Result of checking access to the one strongly matched volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access<'a> {
    /// The entry path was accessible at the time of the check.
    Accessible,
    /// Access was denied, unanswered, or could not be verified.
    Limited(&'a str),
}

/// Aggregated correspondence evidence from the F08 matcher for one catalog.
///
/// `NoMatch` must include any unresolved volume with the same or unknown
/// identity kind. Display names and mount paths are never identity evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matches<'a> {
    /// The catalog has no usable recorded identity.
    NoIdentity,
    /// Multiple candidate volumes correspond to the recorded identity.
    Ambiguous,
    /// Only weak identity evidence matches.
    WeakOnly,
    /// Exactly one strong identity corresponds to a volume, established by
    /// the F08 matcher for the supplied enumeration. The caller must not
    /// promote an uncertain or multiple correspondence to this variant.
    UniqueStrong(Access<'a>),
    /// No strong or weak match was established.
    NoMatch {
        /// At least one relevant volume could not be compared.
        unresolved: bool,
    },
}

/// A per-catalog result at the time of the volume check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status<'a> {
    /// No check has been made in this session.
    Unchecked,
    /// One strong match exists and its entry path was accessible.
    Connected,
    /// One strong match exists, but entry-path access is not confirmed.
    AccessLimited(&'a str),
    /// A complete, conclusive check established that no volume matches.
    Disconnected,
    /// The evidence cannot establish connected or disconnected status.
    Unverifiable(Reason),
}

/// Neutral explanation for a status that cannot be determined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    EnumerationUnavailable,
    IdentityUnknown,
    VolumeAmbiguous,
    WeakIdentity,
    IncompleteEvidence,
    Cancelled,
}

/// Applies `catalog.link-status` revision 1 to one catalog's check evidence.
///
/// A partial enumeration can confirm a unique strong match, but cannot prove
/// absence. The caller must not call this with an unclassified or fabricated
/// match: these decisions do not perform F08 identity comparison themselves.
#[must_use]
pub fn decide(enumeration: Enumeration, matches: Matches<'_>) -> Status<'_> {
    if enumeration == Enumeration::Unavailable {
        return Status::Unverifiable(Reason::EnumerationUnavailable);
    }
    match matches {
        Matches::NoIdentity => Status::Unverifiable(Reason::IdentityUnknown),
        Matches::Ambiguous => Status::Unverifiable(Reason::VolumeAmbiguous),
        Matches::WeakOnly => Status::Unverifiable(Reason::WeakIdentity),
        Matches::UniqueStrong(Access::Accessible) => Status::Connected,
        Matches::UniqueStrong(Access::Limited(reason)) => Status::AccessLimited(reason),
        Matches::NoMatch { unresolved: true } => Status::Unverifiable(Reason::IncompleteEvidence),
        Matches::NoMatch { unresolved: false } if enumeration == Enumeration::Partial => {
            Status::Unverifiable(Reason::IncompleteEvidence)
        }
        Matches::NoMatch { unresolved: false } => Status::Disconnected,
    }
}

/// Leaves an unfinished per-catalog check unverified after cancellation.
#[must_use]
pub fn cancelled() -> Status<'static> {
    Status::Unverifiable(Reason::Cancelled)
}

#[cfg(test)]
mod tests;
