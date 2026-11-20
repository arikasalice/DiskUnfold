#![cfg(target_os = "macos")]
//! #225: after a volume is force-ejected, reads below it report `scan.disconnected`,
//! while an entry that vanished from a live volume does not.
mod disk_image;
mod machine;

use du_core_contracts::{
    ports::PortError,
    scan_observation::{DirectoryObservations, ObservedEntry, OpenedScopeRoot, ScopeRootPort},
};
use du_platform_os::macos_observation::MacScopeRoots;
use std::{fs, os::unix::ffi::OsStrExt, path::Path, process::Command};

type TestResult = Result<(), Box<dyn std::error::Error>>;

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

fn find<'a>(entries: &'a [ObservedEntry], name: &[u8]) -> Result<&'a ObservedEntry, String> {
    entries
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| format!("missing {}", String::from_utf8_lossy(name)))
}

fn disconnected(error: &PortError) -> bool {
    error.reason_key == "scan.disconnected"
}

#[test]
fn reads_after_a_forced_eject_report_a_disconnection() -> TestResult {
    let work = tempfile::tempdir()?;
    let base = fs::canonicalize(work.path())?;
    let mount = base.join("mnt");
    let _attached = match disk_image::attach(&base.join("image.dmg"), &mount, "APFS", "8m") {
        Ok(attached) => attached,
        Err(reason) => {
            return machine::skip(&format!("no disk image could be attached: {reason}"));
        }
    };
    for name in ["kept", "vanishing"] {
        fs::create_dir(mount.join(name))?;
        fs::write(mount.join(name).join("file"), b"x")?;
    }
    let mut root = open(&mount)?;
    let entries = all(&mut *root.directory)?;

    // On a live volume, an entry removed after listing is not a disconnection.
    fs::remove_dir_all(mount.join("vanishing"))?;
    let vanished = root
        .directory
        .open_child(find(&entries, b"vanishing")?)
        .err()
        .ok_or("a removed folder was opened")?;
    assert!(!disconnected(&vanished), "{vanished:?}");

    let kept = find(&entries, b"kept")?;
    let mut child = root
        .directory
        .open_child(kept)
        .map_err(|e| format!("{e:?}"))?;
    let ejected = Command::new("hdiutil")
        .args(["detach", "-force", "-quiet"])
        .arg(&mount)
        .status()?;
    if !ejected.success() {
        return machine::skip("the image could not be force-ejected");
    }
    // The already open folder and a new open below the ejected volume both say so.
    let read = child
        .next_batch()
        .err()
        .ok_or("an ejected folder was read")?;
    assert!(disconnected(&read), "{read:?}");
    let reopened = root
        .directory
        .open_child(kept)
        .err()
        .ok_or("an ejected folder was opened")?;
    assert!(disconnected(&reopened), "{reopened:?}");
    Ok(())
}
