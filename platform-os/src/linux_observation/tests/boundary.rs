//! Races, permissions and mount boundaries observed on real directories.
use super::{TestResult, drain, entry, fail, link_creation_is_unavailable, open};
use du_core_contracts::scan_observation::ObservedValue;
use du_core_contracts::{
    ports::PortErrorKind,
    scan_observation::{
        FilterDecision, FilteredBatch, MetadataObservation, ObservedEntry, ObservedKind,
    },
};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
};

/// A mount point: the mounted root's identity and kind only, without size,
/// times, allocation or space facts.
fn mount_point(item: &ObservedEntry) -> bool {
    item.metadata.as_ref().is_ok_and(|meta| {
        meta.inode != 0
            && meta.kind == ObservedKind::MountPoint
            && meta.size.is_err()
            && meta.modified_at.known().is_none()
            && meta.native_modified_at.is_none()
            && meta.allocated.known().is_none()
            && meta.link_count.known().is_none()
            && meta.space_identity.known().is_none()
    })
}

/// A mount point below a directory on `parent_device`, on another device.
fn outside(item: &ObservedEntry, parent_device: u64) -> bool {
    mount_point(item)
        && item
            .metadata
            .as_ref()
            .is_ok_and(|meta| meta.device != parent_device)
}

/// Claims the hidden mount point is an ordinary directory on the parent volume.
fn forged(item: &ObservedEntry, device: u64) -> ObservedEntry {
    ObservedEntry {
        metadata: Ok(MetadataObservation {
            kind: ObservedKind::Directory,
            device,
            inode: item.enumerated_inode,
            size: Ok(0),
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            space_identity: ObservedValue::NotApplicable,
        }),
        ..item.clone()
    }
}

#[test]
fn reports_vanished_and_replaced_entries_per_item() -> TestResult {
    let temp = tempfile::tempdir()?;
    let base = temp.path();
    for name in ["gone", "swap", "keep"] {
        fs::write(base.join(name), b"old")?;
    }
    fs::create_dir(base.join("dir"))?;
    let mut root = open(base)?;
    // The filter runs after enumeration and before the metadata query.
    let link_created = std::cell::Cell::new(false);
    let mut items = Vec::new();
    let mut race = |name: &[u8]| {
        let changed = match name {
            b"gone" => fs::remove_file(base.join("gone")),
            b"swap" => fs::rename(base.join("swap"), base.join("saved"))
                .and_then(|()| fs::write(base.join("swap"), b"new")),
            b"dir" => {
                fs::rename(base.join("dir"), base.join("dir-old")).and_then(|()| {
                    match symlink("dir-old", base.join("dir")) {
                        Ok(()) => {
                            link_created.set(true);
                            Ok(())
                        }
                        Err(error) if link_creation_is_unavailable(&error) => {
                            eprintln!("skipped Linux symlink replacement subcase: {error}");
                            Ok(())
                        }
                        Err(error) => Err(error),
                    }
                })
            }
            _ => Ok(()),
        };
        if changed.is_ok() {
            FilterDecision::Admit
        } else {
            FilterDecision::Undecidable
        }
    };
    loop {
        match root.directory.next_filtered(&mut race).map_err(fail)? {
            FilteredBatch::Entries(batch) => items.extend(batch),
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            FilteredBatch::Undecidable => return Err("fixture change failed".into()),
        }
    }
    let gone = entry(&items, b"gone")?;
    assert_ne!(gone.enumerated_inode, 0);
    assert_eq!(
        gone.metadata.as_ref().err().map(|e| e.kind.clone()),
        Some(PortErrorKind::NotFound)
    );
    let swap = entry(&items, b"swap")?;
    let swapped = swap.metadata.as_ref().map_err(|e| fail(e.clone()))?;
    assert_ne!(swap.enumerated_inode, swapped.inode);
    let dir = entry(&items, b"dir")?;
    assert_eq!(dir.enumerated_kind, ObservedKind::Directory);
    if link_created.get() {
        assert_eq!(
            dir.metadata.as_ref().map(|m| m.kind.clone()),
            Ok(ObservedKind::Link)
        );
        assert!(root.directory.open_child(dir).is_err());
    } else {
        assert_eq!(
            dir.metadata.as_ref().err().map(|error| error.kind.clone()),
            Some(PortErrorKind::NotFound)
        );
    }
    let keep = entry(&items, b"keep")?;
    assert_eq!(
        keep.metadata.as_ref().map(|m| m.inode),
        Ok(keep.enumerated_inode)
    );
    Ok(())
}

#[test]
fn replaced_directory_is_not_opened_through_a_stale_entry() -> TestResult {
    let temp = tempfile::tempdir()?;
    let base = temp.path();
    fs::create_dir(base.join("child"))?;
    let mut root = open(base)?;
    let entries = drain(&mut *root.directory)?;
    let child = entry(&entries, b"child")?;
    fs::rename(base.join("child"), base.join("child-old"))?;
    fs::create_dir(base.join("child"))?;
    assert_eq!(
        root.directory.open_child(child).err().map(|e| e.kind),
        Some(PortErrorKind::Unsupported)
    );
    fs::remove_dir(base.join("child"))?;
    if let Err(error) = symlink("child-old", base.join("child")) {
        if link_creation_is_unavailable(&error) {
            eprintln!("skipped Linux stale-link subcase after creation failed: {error}");
            return Ok(());
        }
        return Err(error.into());
    }
    assert!(root.directory.open_child(child).is_err());
    Ok(())
}

/// Requires a non-root user; root bypasses the permission bits under test.
#[test]
fn denied_directory_affects_only_its_own_subtree() -> TestResult {
    if std::env::var_os("DU_TEST_UNPRIVILEGED").is_none() {
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let base = temp.path();
    for name in ["locked", "list-only", "open"] {
        fs::create_dir(base.join(name))?;
        fs::write(base.join(name).join("inner"), b"x")?;
    }
    fs::set_permissions(base.join("locked"), fs::Permissions::from_mode(0o000))?;
    fs::set_permissions(base.join("list-only"), fs::Permissions::from_mode(0o444))?;
    let outcome = (|| -> TestResult {
        let mut root = open(base)?;
        let entries = drain(&mut *root.directory)?;
        let locked = entry(&entries, b"locked")?;
        assert_eq!(
            locked.metadata.as_ref().map(|m| m.kind.clone()),
            Ok(ObservedKind::Directory)
        );
        assert_eq!(
            root.directory.open_child(locked).err().map(|e| e.kind),
            Some(PortErrorKind::PermissionDenied)
        );
        let mut listed = root
            .directory
            .open_child(entry(&entries, b"list-only")?)
            .map_err(fail)?;
        let unreadable = drain(&mut *listed)?;
        assert_eq!(
            entry(&unreadable, b"inner")?
                .metadata
                .as_ref()
                .err()
                .map(|e| e.kind.clone()),
            Some(PortErrorKind::PermissionDenied)
        );
        let mut visible = root
            .directory
            .open_child(entry(&entries, b"open")?)
            .map_err(fail)?;
        assert!(drain(&mut *visible)?[0].metadata.is_ok());
        let inside = open(&base.join("locked").join("inner")).err();
        assert!(inside.is_some_and(|error| error.contains("PermissionDenied")));
        Ok(())
    })();
    for name in ["locked", "list-only"] {
        fs::set_permissions(base.join(name), fs::Permissions::from_mode(0o700))?;
    }
    outcome
}

/// Uses the system mounts below `/` (for example `/proc`) as boundaries.
#[test]
fn system_mount_points_report_their_own_volume_and_are_not_entered() -> TestResult {
    let root_device = fs::metadata("/")?.dev();
    let mounted: Vec<&[u8]> = [&b"proc"[..], b"sys", b"dev"]
        .into_iter()
        .filter(|name| {
            let path = Path::new("/").join(std::str::from_utf8(name).unwrap_or("."));
            fs::metadata(path).is_ok_and(|meta| meta.is_dir() && meta.dev() != root_device)
        })
        .collect();
    if mounted.is_empty() {
        eprintln!("F02_LINUX no separate system mount below /; boundary case not observed");
        return Ok(());
    }
    let mut system = open(Path::new("/"))?;
    let mut items = Vec::new();
    let mut include = |name: &[u8]| {
        if mounted.contains(&name) {
            FilterDecision::Admit
        } else {
            FilterDecision::Exclude
        }
    };
    loop {
        match system.directory.next_filtered(&mut include).map_err(fail)? {
            FilteredBatch::Entries(batch) => items.extend(batch),
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            FilteredBatch::Undecidable => return Err("undecidable".into()),
        }
    }
    assert_eq!(items.len(), mounted.len());
    for item in &items {
        assert!(outside(item, root_device), "{:?}", item.metadata);
        let path = Path::new("/").join(std::str::from_utf8(&item.name)?);
        assert_eq!(
            item.metadata.as_ref().map(|meta| meta.device),
            Ok(fs::metadata(path)?.dev())
        );
        assert!(system.directory.open_child(item).is_err());
        assert!(
            system
                .directory
                .open_child(&forged(item, root_device))
                .is_err()
        );
    }
    eprintln!("F02_LINUX system_mount_boundaries={}", items.len());
    Ok(())
}

/// Names of the mounts directly below `/proc`, from this process's mount table.
fn proc_children() -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let table = fs::read_to_string("/proc/self/mountinfo")?;
    let mut names: Vec<Vec<u8>> = table
        .lines()
        .filter_map(|line| line.split(' ').nth(4)?.strip_prefix("/proc/"))
        .filter(|name| !name.is_empty() && !name.contains(['/', '\\']))
        .map(|name| name.as_bytes().to_vec())
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Uses the mounts a container runtime places below `/proc`: directories bound
/// from `/proc` itself (same device, same inode as the covered directory),
/// tmpfs directories and file mounts such as `/proc/kcore`.
#[test]
fn bind_mounts_below_proc_are_mount_points_and_file_mounts_are_not() -> TestResult {
    let proc_device = fs::metadata("/proc")?.dev();
    let names = proc_children()?;
    if names.is_empty() {
        eprintln!("F02_LINUX no mount below /proc; bind mount case not observed");
        return Ok(());
    }
    let mut proc = open(Path::new("/proc"))?;
    let mut include = |name: &[u8]| {
        if names.iter().any(|candidate| candidate == name) {
            FilterDecision::Admit
        } else {
            FilterDecision::Exclude
        }
    };
    let mut items = Vec::new();
    loop {
        match proc.directory.next_filtered(&mut include).map_err(fail)? {
            FilteredBatch::Entries(batch) => items.extend(batch),
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            FilteredBatch::Undecidable => return Err("undecidable".into()),
        }
    }
    let (mut same_device, mut other_device, mut files) = (0, 0, 0);
    for item in &items {
        let path = Path::new("/proc").join(std::str::from_utf8(&item.name)?);
        let mounted = fs::metadata(&path)?;
        if !mounted.is_dir() {
            assert!(!mount_point(item), "{path:?}: {:?}", item.metadata);
            files += 1;
            continue;
        }
        assert!(mount_point(item), "{path:?}: {:?}", item.metadata);
        assert_eq!(
            item.metadata.as_ref().map(|meta| meta.device),
            Ok(mounted.dev())
        );
        assert!(proc.directory.open_child(item).is_err());
        assert!(
            proc.directory
                .open_child(&forged(item, proc_device))
                .is_err()
        );
        if mounted.dev() == proc_device {
            same_device += 1;
        } else {
            other_device += 1;
        }
    }
    eprintln!(
        "F02_LINUX proc_mounts same_device_dirs={same_device} other_device_dirs={other_device} files={files}"
    );
    Ok(())
}

/// Uses a file system the test script mounted at `$DU_TEST_MOUNT_PARENT/mnt`
/// and, when present, a bind mount of another directory of the parent's own
/// device at `$DU_TEST_MOUNT_PARENT/bind`.
#[test]
fn prepared_mount_point_is_a_boundary() -> TestResult {
    let Some(parent) = std::env::var_os("DU_TEST_MOUNT_PARENT") else {
        return Ok(());
    };
    let parent = Path::new(&parent);
    let mut root = open(parent)?;
    let entries = drain(&mut *root.directory)?;
    let mount = entry(&entries, b"mnt")?;
    let device = fs::metadata(parent)?.dev();
    assert!(outside(mount, device), "{:?}", mount.metadata);
    assert!(root.directory.open_child(mount).is_err());
    assert!(root.directory.open_child(&forged(mount, device)).is_err());
    let inner = open(&parent.join("mnt"))?;
    assert_ne!(inner.volume_id, root.volume_id);
    if let Some(ids) = inner.directory.ancestor_file_ids() {
        assert!(ids.is_empty(), "a mount root has no same-mount ancestors");
    }
    if let Ok(bind) = entry(&entries, b"bind") {
        let mounted = fs::metadata(parent.join("bind"))?;
        assert!(mount_point(bind), "{:?}", bind.metadata);
        assert_eq!(mounted.dev(), device);
        assert_eq!(
            bind.metadata.as_ref().map(|meta| (meta.device, meta.inode)),
            Ok((device, mounted.ino()))
        );
        assert_ne!(bind.enumerated_inode, mounted.ino());
        assert!(root.directory.open_child(bind).is_err());
        assert!(root.directory.open_child(&forged(bind, device)).is_err());
        eprintln!("F02_LINUX prepared_bind_mount=checked");
    }
    eprintln!("F02_LINUX prepared_mount_boundary=checked");
    Ok(())
}
