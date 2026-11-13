use du_snapshot_store::catalog_status::{
    Access, Enumeration, Matches, Reason, Status, cancelled, decide,
};

#[test]
fn library_boundary_consumes_correspondence_without_inspecting_names() {
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
            decide(enumeration, Matches::Ambiguous),
            Status::Unverifiable(Reason::VolumeAmbiguous)
        );
        assert_eq!(
            decide(enumeration, Matches::WeakOnly),
            Status::Unverifiable(Reason::WeakIdentity)
        );
    }
    assert_eq!(
        decide(Enumeration::Partial, Matches::NoMatch { unresolved: false }),
        Status::Unverifiable(Reason::IncompleteEvidence)
    );
    assert_eq!(cancelled(), Status::Unverifiable(Reason::Cancelled));
}
