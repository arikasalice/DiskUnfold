#![cfg(target_os = "macos")]
//! A disk image attached below a temporary folder, as a real mount point.
//!
//! `hdiutil` attaches an image without privileges. A runner without it, or
//! one that refuses the attach, prints the reason and skips the checks.
use du_core_contracts::scan_observation::{
    DirectoryObservations, ObservedEntry, ObservedKind, OpenedScopeRoot, ScopeRootPort,
};
use du_platform_os::macos_observation::MacScopeRoots;
use std::{
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    process::Command,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// An attached image; dropping it detaches the image.
struct Attached {
    mount: PathBuf,
}

impl Drop for Attached {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil")
            .args(["detach", "-force", "-quiet"])
            .arg(&self.mount)
            .status();
    }
}

fn run(command: &mut Command) -> Result<(), String> {
    match command.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// Creates an 8 MiB APFS image in `work` and attaches it at `mount`.
fn attach(work: &Path, mount: &Path) -> Result<Attached, String> {
    let image = work.join("image.dmg");
    run(Command::new("hdiutil")
        .args([
            "create", "-quiet", "-size", "8m", "-fs", "APFS", "-volname", "DUMount",
        ])
        .arg(&image))?;
    fs::create_dir(mount).map_err(|e| e.to_string())?;
    run(Command::new("hdiutil")
        .args([
            "attach",
            "-quiet",
            "-nobrowse",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(mount)
        .arg(&image))?;
    Ok(Attached {
        mount: mount.to_path_buf(),
    })
}

fn open(path: &Path) -> Result<OpenedScopeRoot, String> {
    MacScopeRoots
        .open_root(path.as_os_str().as_bytes())
        .map_err(|e| format!("{e:?}"))
}

fn all(directory: &mut dyn DirectoryObservations) -> Result<Vec<ObservedEntry>, String> {
    let mut entries = Vec::new();
    loop {
        let batch = directory.next_batch().map_err(|e| format!("{e:?}"))?;
        if batch.is_empty() {
            return Ok(entries);
        }
        entries.extend(batch);
    }
}

#[test]
fn a_mount_point_is_listed_with_the_covered_identity_and_never_entered() -> TestResult {
    let work = tempfile::tempdir()?;
    let base = fs::canonicalize(work.path())?.join("scope");
    fs::create_dir(&base)?;
    let mount = base.join("mnt");
    let attached = match attach(work.path(), &mount) {
        Ok(attached) => attached,
        Err(reason) => {
            eprintln!("skipped: no disk image could be attached: {reason}");
            return Ok(());
        }
    };
    fs::write(mount.join("inside"), b"inside")?;
    let mut root = open(&base)?;
    let identity = root
        .directory
        .observed_identity()
        .ok_or("the root reported no identity")?;
    let entries = all(&mut *root.directory)?;
    let entry = entries
        .iter()
        .find(|entry| entry.name == b"mnt")
        .ok_or("missing mount point")?;
    let meta = entry.metadata.as_ref().map_err(|e| format!("{e:?}"))?;
    let mounted = fs::metadata(&mount)?;
    eprintln!(
        "F02_MACOS_MOUNT record_device={} parent_device={} enumerated_inode={} kind={:?} mounted_root_device={} mounted_root_inode={}",
        meta.device,
        identity.device,
        entry.enumerated_inode,
        meta.kind,
        mounted.dev(),
        mounted.ino()
    );
    assert_eq!(entry.enumerated_kind, ObservedKind::MountPoint);
    assert_eq!(meta.kind, ObservedKind::MountPoint);
    // The record describes the covered directory on the parent's device.
    assert_eq!(meta.device, identity.device);
    assert_eq!(meta.inode, entry.enumerated_inode);
    assert_ne!((meta.device, meta.inode), (mounted.dev(), mounted.ino()));
    assert!(meta.size.is_err());
    assert!(meta.modified_at.is_none() && meta.allocated.is_none());
    assert!(meta.link_count.is_none() && meta.space_identity.is_none());
    assert!(root.directory.open_child(entry).is_err());
    // The mount itself opens as the root of another volume.
    let volume = open(&mount)?;
    assert_ne!(volume.volume_id, root.volume_id);
    assert_eq!(volume.directory.ancestor_file_ids(), Some(&[][..]));
    drop(volume);
    drop(root);
    drop(attached);
    Ok(())
}
