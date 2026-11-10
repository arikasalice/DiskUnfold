use super::*;

#[test]
fn session_starts_unchecked_and_cancellation_does_not_mean_disconnected() {
    assert_ne!(Status::Unchecked, cancelled());
    assert_eq!(cancelled(), Status::Unverifiable(Reason::Cancelled));
}

#[test]
fn enumeration_failure_precedes_even_apparent_matches() {
    for matches in [
        Matches::NoIdentity,
        Matches::UniqueStrong(Access::Accessible),
        Matches::NoMatch { unresolved: false },
    ] {
        assert_eq!(
            decide(Enumeration::Unavailable, matches),
            Status::Unverifiable(Reason::EnumerationUnavailable)
        );
    }
}

#[test]
fn missing_ambiguous_and_weak_identities_are_unverifiable() {
    for enumeration in [Enumeration::Complete, Enumeration::Partial] {
        for (matches, reason) in [
            (Matches::NoIdentity, Reason::IdentityUnknown),
            (Matches::Ambiguous, Reason::VolumeAmbiguous),
            (Matches::WeakOnly, Reason::WeakIdentity),
        ] {
            assert_eq!(decide(enumeration, matches), Status::Unverifiable(reason));
        }
    }
}

#[test]
fn strong_match_uses_current_access_even_with_partial_enumeration() {
    for enumeration in [Enumeration::Complete, Enumeration::Partial] {
        assert_eq!(
            decide(enumeration, Matches::UniqueStrong(Access::Accessible)),
            Status::Connected
        );
        assert_eq!(
            decide(
                enumeration,
                Matches::UniqueStrong(Access::Limited("access.denied"))
            ),
            Status::AccessLimited("access.denied")
        );
        assert_eq!(
            decide(
                enumeration,
                Matches::UniqueStrong(Access::Limited("access.unanswered"))
            ),
            Status::AccessLimited("access.unanswered")
        );
    }
}

#[test]
fn only_complete_conclusive_absence_is_disconnected() {
    assert_eq!(
        decide(
            Enumeration::Complete,
            Matches::NoMatch { unresolved: false }
        ),
        Status::Disconnected
    );
    for enumeration in [Enumeration::Complete, Enumeration::Partial] {
        assert_eq!(
            decide(enumeration, Matches::NoMatch { unresolved: true }),
            Status::Unverifiable(Reason::IncompleteEvidence)
        );
    }
    assert_eq!(
        decide(Enumeration::Partial, Matches::NoMatch { unresolved: false }),
        Status::Unverifiable(Reason::IncompleteEvidence)
    );
}
