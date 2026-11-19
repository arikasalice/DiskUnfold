#![cfg(target_os = "linux")]
//! btrfs subvolumes observed through the real Linux adapter.
//!
//! The ignored test reads a btrfs volume that the Linux CI step mounts at
//! `$DU_TEST_BTRFS_ROOT` through a loop device. It holds the subvolumes
//! `first`, `second` and `second/nested` (IDs 256, 257 and 258) and the
//! folder `plain`.
use du_core_contracts::scan_observation::{
    AnchoredDirectory, ObservedEntry, ObservedKind, OpenedScopeRoot, ScopeRootPort,
};
use du_platform_os::linux_observation::LinuxScopeRoots;
use std::{
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn open(path: &Path) -> Result<OpenedScopeRoot, String> {
    LinuxScopeRoots
        .open_root(path.as_os_str().as_bytes())
        .map_err(|error| format!("{error:?}"))
}

fn drain(directory: &mut dyn AnchoredDirectory) -> Result<Vec<ObservedEntry>, String> {
    let mut all = Vec::new();
    loop {
        let batch = directory
            .next_batch()
            .map_err(|error| format!("{error:?}"))?;
        if batch.is_empty() {
            return Ok(all);
        }
        all.extend(batch);
    }
}

fn find<'a>(entries: &'a [ObservedEntry], name: &str) -> Result<&'a ObservedEntry, String> {
    entries
        .iter()
        .find(|entry| entry.name == name.as_bytes())
        .ok_or_else(|| format!("{name} was not enumerated"))
}

/// Prints what enumeration and `statx` reported for each named subvolume.
fn report(entries: &[ObservedEntry], parent: &Path, names: &[&str]) -> TestResult {
    let parent_device = fs::metadata(parent)?.dev();
    for name in names {
        let entry = find(entries, name)?;
        let stat = fs::symlink_metadata(parent.join(name))?;
        eprintln!(
            "F02_LINUX_BTRFS name={name} parent_device={parent_device} stat_device={} \
             stat_inode={} enumerated_kind={:?} enumerated_inode={} observed={:?}",
            stat.dev(),
            stat.ino(),
            entry.enumerated_kind,
            entry.enumerated_inode,
            entry
                .metadata
                .as_ref()
                .map(|meta| (meta.kind.clone(), meta.device, meta.inode)),
        );
    }
    Ok(())
}

/// A subvolume meets the conditions under which the collector leaves a
/// directory outside the boundary without comparing its enumerated inode: a
/// directory in enumeration and metadata, on another device than its parent.
/// Its own device and root inode are reported with no other facts, and it is
/// never entered. The enumerated inode is the subvolume ID and is not checked.
fn check_subvolume(
    directory: &dyn AnchoredDirectory,
    entries: &[ObservedEntry],
    parent: &Path,
    name: &str,
) -> TestResult {
    let entry = find(entries, name)?;
    let stat = fs::symlink_metadata(parent.join(name))?;
    let meta = entry
        .metadata
        .as_ref()
        .map_err(|error| format!("{name}: {error:?}"))?;
    assert_eq!(entry.enumerated_kind, ObservedKind::Directory, "{name}");
    assert_eq!(meta.kind, ObservedKind::Directory, "{name}");
    assert_ne!(meta.device, fs::metadata(parent)?.dev(), "{name}");
    assert_eq!(
        (meta.device, meta.inode),
        (stat.dev(), stat.ino()),
        "{name}"
    );
    assert!(meta.size.is_err(), "{name}");
    assert!(meta.modified_at.is_none(), "{name}");
    assert!(meta.allocated.is_none(), "{name}");
    assert!(meta.link_count.is_none(), "{name}");
    assert!(meta.space_identity.is_none(), "{name}");
    assert!(directory.open_child(entry).is_err(), "{name} was entered");
    Ok(())
}

#[test]
#[ignore = "requires the btrfs volume that the Linux CI step mounts"]
fn btrfs_subvolumes_are_other_volumes_that_are_never_entered() -> TestResult {
    let Some(root) = std::env::var_os("DU_TEST_BTRFS_ROOT") else {
        eprintln!("skipping: DU_TEST_BTRFS_ROOT is not set");
        return Ok(());
    };
    let root = Path::new(&root);
    let release = fs::read_to_string("/proc/sys/kernel/osrelease")?;
    eprintln!("F02_LINUX_BTRFS kernel={}", release.trim());
    let mut top = open(root)?;
    let entries = drain(&mut *top.directory)?;
    let second = root.join("second");
    let mut inner = open(&second)?;
    let nested = drain(&mut *inner.directory)?;
    report(&entries, root, &["first", "second"])?;
    report(&nested, &second, &["nested"])?;

    let plain = find(&entries, "plain")?;
    let meta = plain
        .metadata
        .as_ref()
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(meta.kind, ObservedKind::Directory);
    assert_eq!(meta.device, fs::metadata(root)?.dev());
    assert_eq!(meta.inode, plain.enumerated_inode);
    let mut opened = top
        .directory
        .open_child(plain)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(drain(&mut *opened)?.len(), 1);

    for name in ["first", "second"] {
        check_subvolume(&*top.directory, &entries, root, name)?;
    }
    check_subvolume(&*inner.directory, &nested, &second, "nested")?;
    assert_ne!(top.volume_id, inner.volume_id);
    Ok(())
}
