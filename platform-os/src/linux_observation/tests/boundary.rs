//! Races, permissions and mount boundaries observed on real directories.
use super::{TestResult, drain, entry, fail, open};
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

fn unresolvable(item: &ObservedEntry) -> bool {
    item.metadata.as_ref().err().is_some_and(|error| {
        error.kind == PortErrorKind::Unsupported && error.reason_key == "path.unresolvable"
    })
}

/// Claims the hidden mount point is an ordinary directory on the parent volume.
fn forged(item: &ObservedEntry, device: u64) -> ObservedEntry {
    ObservedEntry {
        metadata: Ok(MetadataObservation {
            kind: ObservedKind::Directory,
            device,
            inode: item.enumerated_inode,
            size: Ok(0),
            modified_at: None,
            native_modified_at: None,
            allocated: None,
            link_count: None,
            space_identity: None,
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
    let mut race = |name: &[u8]| {
        let changed = match name {
            b"gone" => fs::remove_file(base.join("gone")),
            b"swap" => fs::rename(base.join("swap"), base.join("saved"))
                .and_then(|()| fs::write(base.join("swap"), b"new")),
            b"dir" => fs::rename(base.join("dir"), base.join("dir-old"))
                .and_then(|()| symlink("dir-old", base.join("dir"))),
            _ => Ok(()),
        };
        if changed.is_ok() {
            FilterDecision::Admit
        } else {
            FilterDecision::Undecidable
        }
    };
    let mut items = Vec::new();
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
    assert_eq!(
        dir.metadata.as_ref().map(|m| m.kind.clone()),
        Ok(ObservedKind::Link)
    );
    assert!(root.directory.open_child(dir).is_err());
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
    symlink("child-old", base.join("child"))?;
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
fn system_mount_points_are_neither_observed_nor_entered() -> TestResult {
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
        assert!(unresolvable(item), "{:?}", item.metadata);
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

/// Uses a file system the test script mounted at `$DU_TEST_MOUNT_PARENT/mnt`.
#[test]
fn prepared_mount_point_is_a_boundary() -> TestResult {
    let Some(parent) = std::env::var_os("DU_TEST_MOUNT_PARENT") else {
        return Ok(());
    };
    let parent = Path::new(&parent);
    let mut root = open(parent)?;
    let entries = drain(&mut *root.directory)?;
    let mount = entry(&entries, b"mnt")?;
    assert!(unresolvable(mount), "{:?}", mount.metadata);
    let device = fs::metadata(parent)?.dev();
    assert!(root.directory.open_child(&forged(mount, device)).is_err());
    let inner = open(&parent.join("mnt"))?;
    assert_ne!(inner.volume_id, root.volume_id);
    if let Some(ids) = inner.directory.ancestor_file_ids() {
        assert!(ids.is_empty(), "a mount root has no same-mount ancestors");
    }
    eprintln!("F02_LINUX prepared_mount_boundary=checked");
    Ok(())
}
