use super::*;
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{DirectoryObservations, HandleIdentity},
};

fn entry(kind: ObservedKind, device: u64, inode: u64) -> ObservedEntry {
    ObservedEntry {
        name: b"child".to_vec(),
        enumerated_inode: inode,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device,
            inode,
            size: Ok(0),
            modified_at: None,
            native_modified_at: None,
            allocated: None,
            link_count: None,
            space_identity: None,
        }),
    }
}

struct Root(Option<HandleIdentity>);
impl DirectoryObservations for Root {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for Root {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
    fn observed_identity(&self) -> Option<HandleIdentity> {
        self.0.clone()
    }
}

#[test]
fn rows_match_in_the_published_order() {
    let visited = Visited::default();
    let parent = Boundary::Volume(1);
    let link_elsewhere = entry(ObservedKind::Link, 2, 5);
    assert_eq!(
        decide(true, &link_elsewhere, parent, &visited),
        Admission::Excluded
    );
    assert_eq!(
        decide(false, &link_elsewhere, parent, &visited),
        Admission::UntraversedLink
    );
    let other_volume = entry(ObservedKind::Directory, 2, 6);
    assert_eq!(
        decide(false, &other_volume, parent, &visited),
        Admission::OutsideBoundary
    );
    let directory = entry(ObservedKind::Directory, 1, 7);
    assert_eq!(
        decide(false, &directory, parent, &visited),
        Admission::Enter
    );
    assert_eq!(
        decide(false, &directory, parent, &visited),
        Admission::Revisit
    );
    assert_eq!(
        decide(false, &directory, Boundary::Unknown, &visited),
        Admission::Revisit
    );
    let unseen = entry(ObservedKind::Directory, 1, 8);
    assert_eq!(
        decide(false, &unseen, Boundary::Unknown, &visited),
        Admission::Unverifiable
    );
    assert_eq!(
        decide(false, &unseen, Boundary::Pending, &visited),
        Admission::Unverifiable
    );
    assert_eq!(
        decide(
            false,
            &entry(ObservedKind::File, 1, 9),
            Boundary::Unknown,
            &visited
        ),
        Admission::Record
    );
    assert_eq!(
        decide(false, &entry(ObservedKind::File, 2, 9), parent, &visited),
        Admission::OutsideBoundary
    );
}

#[test]
fn inconsistent_metadata_is_recorded_and_never_entered() {
    let visited = Visited::default();
    let mut changed = entry(ObservedKind::Directory, 1, 7);
    changed.enumerated_inode = 8;
    let mut failed = entry(ObservedKind::Directory, 1, 7);
    failed.metadata = Err(PortError {
        kind: PortErrorKind::PermissionDenied,
        reason_key: "os.query_failed".into(),
        os_code: None,
    });
    for entry in [changed, failed, entry(ObservedKind::Other, 2, 7)] {
        assert_eq!(
            decide(false, &entry, Boundary::Volume(1), &visited),
            Admission::Record
        );
    }
    assert_eq!(
        decide(
            false,
            &entry(ObservedKind::Directory, 1, 7),
            Boundary::Volume(1),
            &visited
        ),
        Admission::Enter
    );
}

#[test]
fn root_identity_and_ancestors_are_reserved() {
    let visited = Visited::default();
    let boundary = visited.observe_root(&Root(Some(HandleIdentity {
        device: 3,
        inode: 30,
        ancestors: vec![10, 20],
    })));
    assert_eq!(boundary, Boundary::Volume(3));
    for inode in [10, 20, 30] {
        assert_eq!(
            decide(
                false,
                &entry(ObservedKind::Directory, 3, inode),
                boundary,
                &visited
            ),
            Admission::Revisit
        );
    }
    assert_eq!(
        decide(
            false,
            &entry(ObservedKind::Directory, 4, 10),
            boundary,
            &visited
        ),
        Admission::OutsideBoundary
    );
    assert_eq!(visited.observe_root(&Root(None)), Boundary::Unknown);
}

#[test]
fn undecidable_exclusion_is_withheld_and_kept() {
    let mut undecided = Vec::new();
    assert_eq!(
        exclusion(FilterDecision::Undecidable, b"unknown", &mut undecided),
        FilterDecision::Exclude
    );
    assert_eq!(
        exclusion(FilterDecision::Admit, b"kept", &mut undecided),
        FilterDecision::Admit
    );
    assert_eq!(
        exclusion(FilterDecision::Exclude, b"skip", &mut undecided),
        FilterDecision::Exclude
    );
    assert_eq!(undecided, [b"unknown".to_vec()]);
}
