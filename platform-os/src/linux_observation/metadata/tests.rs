//! Entry classification from fixed `statx` facts, including cases that a
//! container cannot mount, such as a btrfs subvolume.
use super::{Anchor, Facts, FileType, MetadataObservation, ObservedKind, PortError, classify};
use std::{fs::File, sync::Arc};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DEVICE: u64 = 10;
const MOUNT: u64 = 20;

fn anchor() -> Result<Anchor, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    Ok(Anchor {
        fd: Arc::new(File::open(temp.path())?.into()),
        device: DEVICE,
        mount: Some(MOUNT),
        volume_id: Arc::from(&b"linux:test"[..]),
    })
}

/// A directory on the parent's device and mount, not a mount root.
fn directory() -> Facts {
    Facts {
        file_type: Some(FileType::Directory),
        device: DEVICE,
        inode: Some(256),
        size: Some(4096),
        blocks: Some(8),
        links: Some(2),
        modified: Some((1_700_000_000, 0)),
        mount: Some(MOUNT),
        mount_root: Some(false),
        automount: false,
    }
}

/// Classifies fixed facts; a port error is rendered for the test result.
fn observed(anchor: &Anchor, facts: &Facts) -> Result<MetadataObservation, String> {
    classify(anchor, facts).map_err(|error: PortError| format!("{error:?}"))
}

/// Only the identity and kind of an entry on another volume are reported.
fn other_volume(meta: &MetadataObservation, kind: ObservedKind, device: u64) -> bool {
    meta.kind == kind
        && meta.device == device
        && meta.inode == 256
        && meta.size.is_err()
        && meta.modified_at.is_none()
        && meta.native_modified_at.is_none()
        && meta.allocated.is_none()
        && meta.link_count.is_none()
        && meta.space_identity.is_none()
}

/// A directory whose device alone differs, on the parent's mount and not a
/// mount root (a btrfs subvolume), keeps its own kind under the other-volume
/// rule, so the collector compares its inodes and leaves it outside the
/// boundary of the known parent volume.
#[test]
fn a_directory_on_another_device_without_a_mount_stays_a_directory() -> TestResult {
    let anchor = anchor()?;
    let subvolume = Facts {
        device: DEVICE + 1,
        ..directory()
    };
    let meta = observed(&anchor, &subvolume)?;
    assert!(
        other_volume(&meta, ObservedKind::Directory, DEVICE + 1),
        "{meta:?}"
    );
    // A kernel that reports neither mount IDs nor the mount root attribute.
    let older = Facts {
        mount: None,
        mount_root: None,
        ..subvolume
    };
    let meta = observed(&anchor, &older)?;
    assert!(
        other_volume(&meta, ObservedKind::Directory, DEVICE + 1),
        "{meta:?}"
    );
    Ok(())
}

#[test]
fn a_directory_with_a_mount_on_it_is_a_mount_point() -> TestResult {
    let anchor = anchor()?;
    let cases = [
        // A bind mount of the same device: the mount root and a new mount ID.
        Facts {
            mount: Some(MOUNT + 1),
            mount_root: Some(true),
            ..directory()
        },
        // Only the mount ID shows the mount.
        Facts {
            mount: Some(MOUNT + 1),
            mount_root: None,
            ..directory()
        },
        // Only the mount root attribute shows it.
        Facts {
            mount: None,
            mount_root: Some(true),
            ..directory()
        },
    ];
    for facts in cases {
        let meta = observed(&anchor, &facts)?;
        assert!(
            other_volume(&meta, ObservedKind::MountPoint, DEVICE),
            "{meta:?}"
        );
    }
    let tmpfs = Facts {
        device: DEVICE + 2,
        mount: Some(MOUNT + 2),
        mount_root: Some(true),
        ..directory()
    };
    let meta = observed(&anchor, &tmpfs)?;
    assert!(
        other_volume(&meta, ObservedKind::MountPoint, DEVICE + 2),
        "{meta:?}"
    );
    Ok(())
}

#[test]
fn file_mounts_and_automount_triggers_are_not_mount_points() -> TestResult {
    let anchor = anchor()?;
    let file = Facts {
        file_type: Some(FileType::RegularFile),
        ..directory()
    };
    let same_device_file_mount = Facts {
        mount: Some(MOUNT + 1),
        mount_root: Some(true),
        ..file
    };
    assert!(observed(&anchor, &same_device_file_mount).is_err());
    let trigger = Facts {
        automount: true,
        ..directory()
    };
    assert!(observed(&anchor, &trigger).is_err());
    let other_device_file_mount = Facts {
        device: DEVICE + 3,
        ..same_device_file_mount
    };
    let meta = observed(&anchor, &other_device_file_mount)?;
    assert!(
        other_volume(&meta, ObservedKind::File, DEVICE + 3),
        "{meta:?}"
    );
    let plain = observed(&anchor, &directory())?;
    assert_eq!(
        (plain.kind, plain.size),
        (ObservedKind::Directory, Ok(4096))
    );
    Ok(())
}
