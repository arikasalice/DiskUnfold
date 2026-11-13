#![cfg(windows)]
//! NTFS junction containment checks using an isolated workspace fixture.
use du_core_contracts::scan_observation::ScopeRootPort;
use du_platform_os::windows_observation::WindowsScopeRoots;
use std::{fs, os::windows::ffi::OsStrExt, path::Path, process::Command};

#[test]
fn junction_scope_and_ancestor_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let target = fixture.path().join("target");
    let junction = fixture.path().join("junction");
    fs::create_dir(&target)?;
    fs::create_dir(target.join("inside"))?;
    let status = Command::new("cmd.exe")
        .current_dir(fixture.path())
        .args(["/D", "/C", "mklink", "/J", "junction", "target"])
        .status()?;
    if !status.success() {
        return Err("could not create junction fixture".into());
    }
    let open = |path: &Path| {
        let raw: Vec<_> = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        WindowsScopeRoots.open_root(&raw)
    };
    let target_opened = open(&target.join("inside")).is_ok();
    let junction_rejected = open(&junction).err();
    let nested_rejected = open(&junction.join("inside")).err();
    // Remove only the junction itself; never recursively traverse its target.
    fs::remove_dir(&junction)?;
    assert!(target_opened);
    assert_eq!(
        junction_rejected.ok_or("junction was admitted")?.reason_key,
        "path.unresolvable"
    );
    assert_eq!(
        nested_rejected
            .ok_or("nested junction was admitted")?
            .reason_key,
        "path.unresolvable"
    );
    Ok(())
}
