use super::*;
use du_core_contracts::scan_observation::ObservedValue;
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::HandleIdentity,
    volume::{ConfirmedFirmlink, RootVolumeGroup},
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
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            space_identity: ObservedValue::NotApplicable,
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
fn a_mount_point_is_outside_any_boundary_and_reserves_nothing() {
    let visited = Visited::default();
    // Enumerated as the covered directory, observed as the mounted root.
    let mut mount = entry(ObservedKind::MountPoint, 1, 40);
    mount.enumerated_kind = ObservedKind::Directory;
    mount.enumerated_inode = 41;
    for boundary in [Boundary::Volume(1), Boundary::Volume(2), Boundary::Unknown] {
        assert_eq!(
            decide(false, &mount, at(0, boundary), &visited),
            Admission::OutsideBoundary
        );
    }
    let parent = at(0, Boundary::Volume(1));
    assert_eq!(decide(true, &mount, parent, &visited), Admission::Excluded);
    // Only a directory can be a mount point.
    mount.enumerated_kind = ObservedKind::File;
    assert_eq!(decide(false, &mount, parent, &visited), Admission::Record);
    let same_identity = entry(ObservedKind::Directory, 1, 40);
    assert_eq!(
        decide(false, &same_identity, parent, &visited),
        Admission::Enter
    );
}

#[test]
fn a_mounted_file_is_outside_any_boundary_and_reserves_nothing() {
    let visited = Visited::default();
    // Enumerated as the covered file, observed as the mounted file.
    let mut mounted = entry(ObservedKind::MountedFile, 2, 50);
    mounted.enumerated_kind = ObservedKind::File;
    mounted.enumerated_inode = 51;
    for boundary in [Boundary::Volume(1), Boundary::Volume(2), Boundary::Unknown] {
        assert_eq!(
            decide(false, &mounted, at(0, boundary), &visited),
            Admission::OutsideBoundary
        );
    }
    let parent = at(0, Boundary::Volume(1));
    assert_eq!(
        decide(true, &mounted, parent, &visited),
        Admission::Excluded
    );
    // Only a file can be a mounted file.
    mounted.enumerated_kind = ObservedKind::Directory;
    assert_eq!(decide(false, &mounted, parent, &visited), Admission::Record);
}

#[test]
fn a_directory_on_another_known_volume_is_outside_without_an_inode_match() {
    let visited = Visited::default();
    // btrfs lists a subvolume with its ID; its root reports inode 256.
    let mut subvolume = entry(ObservedKind::Directory, 2, 256);
    subvolume.enumerated_inode = 257;
    let parent = at(0, Boundary::Volume(1));
    assert_eq!(
        decide(false, &subvolume, parent, &visited),
        Admission::OutsideBoundary
    );
    assert_eq!(
        decide(true, &subvolume, parent, &visited),
        Admission::Excluded
    );
    let mut unknown_kind = subvolume.clone();
    unknown_kind.enumerated_kind = ObservedKind::Unknown;
    assert_eq!(
        decide(false, &unknown_kind, parent, &visited),
        Admission::OutsideBoundary
    );
    // An unknown parent volume and the parent's own volume keep the comparison.
    assert_eq!(
        decide(false, &subvolume, at(0, Boundary::Unknown), &visited),
        Admission::Record
    );
    let mut same_volume = entry(ObservedKind::Directory, 1, 256);
    same_volume.enumerated_inode = 257;
    assert_eq!(
        decide(false, &same_volume, parent, &visited),
        Admission::Record
    );
    // A file or link replaced by a directory stays a failure.
    for kind in [ObservedKind::File, ObservedKind::Link] {
        let mut raced = subvolume.clone();
        raced.enumerated_kind = kind;
        assert_eq!(decide(false, &raced, parent, &visited), Admission::Record);
    }
    // Files are not covered by the rule.
    let mut file = entry(ObservedKind::File, 2, 256);
    file.enumerated_inode = 257;
    assert_eq!(decide(false, &file, parent, &visited), Admission::Record);
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

fn volume_group() -> RootVolumeGroup {
    RootVolumeGroup {
        data_mount: b"/System/Volumes/Data".to_vec(),
        system_name: None,
        device: 1,
        data_inode: 20,
        firmlinks: vec![ConfirmedFirmlink {
            source: b"/Users".to_vec(),
            target: b"/System/Volumes/Data/Users".to_vec(),
            device: 1,
            inode: 99,
        }],
    }
}

fn firmlink(enumerated_inode: u64, target_inode: u64) -> ObservedEntry {
    let mut observed = entry(ObservedKind::Firmlink, 1, target_inode);
    observed.enumerated_inode = enumerated_inode;
    observed
}

#[test]
fn only_a_listed_group_firmlink_identity_is_entered_once() {
    let (visited, _) =
        Visited::reserve_roots_with_group(vec![identity(1, 10, &[])], Some(volume_group()));
    let listed = firmlink(40, 99);
    assert_eq!(
        decide(false, &listed, at(0, Boundary::Volume(1)), &visited),
        Admission::Enter
    );
    assert_eq!(
        decide(false, &listed, at(0, Boundary::Volume(1)), &visited),
        Admission::Revisit
    );
    let unlisted = firmlink(41, 100);
    assert_eq!(
        decide(false, &unlisted, at(0, Boundary::Volume(1)), &visited),
        Admission::UntraversedLink
    );
    let without_group = Visited::default();
    assert_eq!(
        decide(false, &listed, at(0, Boundary::Volume(1)), &without_group),
        Admission::UntraversedLink
    );
}

#[test]
fn confirmed_data_mount_enters_with_the_group_device_boundary() {
    let (visited, _) =
        Visited::reserve_roots_with_group(vec![identity(1, 10, &[])], Some(volume_group()));
    let mut mount = entry(ObservedKind::MountPoint, 1, 20);
    mount.enumerated_inode = 40;
    mount.enumerated_kind = ObservedKind::Directory;
    assert_eq!(
        decide(false, &mount, at(0, Boundary::Volume(1)), &visited),
        Admission::Enter
    );
    assert_eq!(
        visited.child_boundary(&mount, Boundary::Volume(1)),
        Boundary::Data(1)
    );
    assert_eq!(
        decide(
            false,
            &mount,
            at(0, Boundary::Volume(1)),
            &Visited::default()
        ),
        Admission::OutsideBoundary
    );
}

#[test]
fn data_target_is_not_reserved_so_firmlink_wins_in_either_order() {
    let data_alias = entry(ObservedKind::Directory, 1, 99);
    let link = firmlink(40, 99);
    let (data_first, _) =
        Visited::reserve_roots_with_group(vec![identity(1, 10, &[])], Some(volume_group()));
    assert_eq!(
        decide(false, &data_alias, at(0, Boundary::Data(1)), &data_first),
        Admission::Revisit
    );
    assert!(!data_first.known(0, 1, 99));
    assert_eq!(
        decide(false, &link, at(0, Boundary::Volume(1)), &data_first),
        Admission::Enter
    );

    let (link_first, _) =
        Visited::reserve_roots_with_group(vec![identity(1, 10, &[])], Some(volume_group()));
    assert_eq!(
        decide(false, &link, at(0, Boundary::Volume(1)), &link_first),
        Admission::Enter
    );
    assert_eq!(
        decide(false, &data_alias, at(0, Boundary::Data(1)), &link_first),
        Admission::Revisit
    );
    assert_eq!(Admission::Revisit.unread(), Some(Unread::Revisit));
}
