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
use std::{
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
    process::Command,
    thread,
    time::Duration,
};

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

/// Whether `mount` is still on another device than its parent, that is, still mounted.
fn mounted(mount: &Path) -> bool {
    let device = |path: &Path| fs::metadata(path).map(|metadata| metadata.dev()).ok();
    match (device(mount), mount.parent().and_then(device)) {
        (Some(own), Some(parent)) => own != parent,
        _ => false,
    }
}

/// Force-detaches the image at `mount`. Disk Arbitration can refuse a forced detach while
/// it is still busy with the volume, as a hosted runner showed (#252), so the detach is
/// tried up to five times, one second apart; a refusal after which the volume is gone
/// counts as done. The error names the last refusal.
fn force_detach(mount: &Path) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 0..5 {
        if attempt > 0 {
            thread::sleep(Duration::from_secs(1));
        }
        match Command::new("hdiutil")
            .args(["detach", "-force"])
            .arg(mount)
            .output()
        {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => last = String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            Err(error) => last = error.to_string(),
        }
        if !mounted(mount) {
            return Ok(());
        }
    }
    Err(last)
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
    if let Err(reason) = force_detach(&mount) {
        return machine::skip(&format!("the image could not be force-ejected: {reason}"));
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
