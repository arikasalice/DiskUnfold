use super::*;
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::HandleIdentity,
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

fn identity(device: u64, inode: u64, ancestors: &[u64]) -> Option<HandleIdentity> {
    Some(HandleIdentity {
        device,
        inode,
        ancestors: ancestors.to_vec(),
    })
}

fn at(root: u32, boundary: Boundary) -> Parent {
    Parent { root, boundary }
}

#[test]
fn rows_match_in_the_published_order() {
    let visited = Visited::default();
    let parent = at(0, Boundary::Volume(1));
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
    let unknown = at(0, Boundary::Unknown);
    assert_eq!(
        decide(false, &directory, unknown, &visited),
        Admission::Revisit
    );
    let unseen = entry(ObservedKind::Directory, 1, 8);
    assert_eq!(
        decide(false, &unseen, unknown, &visited),
        Admission::Unverifiable
    );
    assert_eq!(
        decide(false, &entry(ObservedKind::File, 1, 9), unknown, &visited),
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
    let parent = at(0, Boundary::Volume(1));
    let mut changed = entry(ObservedKind::Directory, 1, 7);
    changed.enumerated_inode = 8;
    let mut failed = entry(ObservedKind::Directory, 1, 7);
    failed.metadata = Err(PortError {
        kind: PortErrorKind::PermissionDenied,
        reason_key: "os.query_failed".into(),
        os_code: None,
    });
    for entry in [changed, failed, entry(ObservedKind::Other, 2, 7)] {
        assert_eq!(decide(false, &entry, parent, &visited), Admission::Record);
    }
    assert_eq!(
        decide(
            false,
            &entry(ObservedKind::Directory, 1, 7),
            parent,
            &visited
        ),
        Admission::Enter
    );
}

#[test]
fn roots_are_reserved_in_order_and_ancestors_block_only_their_root() {
    // Root 1 lies below root 0 (inode 20); root 2 repeats root 0.
    let (visited, starts) = Visited::reserve_roots(vec![
        identity(3, 20, &[10]),
        identity(3, 30, &[20, 10, 15]),
        identity(3, 20, &[10]),
        None,
    ]);
    assert_eq!(
        starts,
        [
            RootStart::Collect(Boundary::Volume(3)),
            RootStart::Collect(Boundary::Volume(3)),
            RootStart::Revisit,
            RootStart::Collect(Boundary::Unknown),
        ]
    );
    let dir = |inode| entry(ObservedKind::Directory, 3, inode);
    let (outer, inner) = (at(0, Boundary::Volume(3)), at(1, Boundary::Volume(3)));
    // Another root, or the root itself, is always a revisit.
    assert_eq!(decide(false, &dir(30), outer, &visited), Admission::Revisit);
    assert_eq!(decide(false, &dir(20), inner, &visited), Admission::Revisit);
    // Root 1's ancestor 15 lies inside root 0, which may enter it.
    assert_eq!(decide(false, &dir(10), outer, &visited), Admission::Revisit);
    assert_eq!(decide(false, &dir(15), inner, &visited), Admission::Revisit);
    assert_eq!(decide(false, &dir(15), outer, &visited), Admission::Enter);
    assert_eq!(
        decide(
            false,
            &entry(ObservedKind::Directory, 4, 10),
            outer,
            &visited
        ),
        Admission::OutsideBoundary
    );
}

#[test]
fn preview_decides_without_reserving() {
    let visited = Visited::default();
    let parent = at(0, Boundary::Volume(1));
    let directory = entry(ObservedKind::Directory, 1, 7);
    assert_eq!(
        preview(false, &directory, parent, &visited),
        Admission::Enter
    );
    assert_eq!(
        decide(false, &directory, parent, &visited),
        Admission::Enter
    );
    assert_eq!(
        preview(false, &directory, parent, &visited),
        Admission::Revisit
    );
    let unseen = entry(ObservedKind::Directory, 1, 8);
    assert_eq!(
        preview(false, &unseen, at(0, Boundary::Unknown), &visited),
        Admission::Unverifiable
    );
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
