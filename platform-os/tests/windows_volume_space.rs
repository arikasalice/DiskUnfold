#![cfg(windows)]
use du_core_contracts::{ports::PortErrorKind, scan_observation::ScopeRootPort};
use du_platform_os::windows_observation::{WindowsScopeRoots, open_verified};
use std::{ffi::OsStr, fs, os::windows::ffi::OsStrExt};

#[test]
fn verified_root_volume_reports_survive_path_replacement() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("scope");
    fs::create_dir(&path)?;
    let raw: Vec<_> = OsStr::new(&path)
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let root = WindowsScopeRoots
        .open_root(&raw)
        .map_err(|e| format!("{e:?}"))?;
    let directory = open_verified(&path).map_err(|e| format!("{e:?}"))?;
    let first_id = directory.identity().map_err(|e| format!("{e:?}"))?.file_id;
    let first = directory
        .volume_space(&root.volume_id)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.volume_id, root.volume_id);
    assert!(first.capacity.total_bytes >= first.capacity.available_bytes);
    assert!(first.actual_free_bytes >= first.capacity.available_bytes);
    assert!(first.actual_total_bytes >= first.actual_free_bytes);
    assert!(first.actual_total_bytes >= first.used_bytes);
    let mut different = root.volume_id.clone();
    different[0] ^= 1;
    let unavailable = directory
        .volume_space(&different)
        .err()
        .ok_or("capacity was returned for the wrong volume")?;
    assert_eq!(unavailable.kind, PortErrorKind::IdentityChanged);
    assert_eq!(unavailable.reason_key, "space.volume_report_unavailable");
    fs::rename(&path, fixture.path().join("original"))?;
    fs::create_dir(&path)?;
    let replacement = open_verified(&path).map_err(|e| format!("{e:?}"))?;
    assert_ne!(
        first_id,
        replacement
            .identity()
            .map_err(|e| format!("{e:?}"))?
            .file_id
    );
    let last = directory
        .volume_space(&root.volume_id)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(last.volume_id, first.volume_id);
    assert!(last.capacity.total_bytes >= last.capacity.available_bytes);
    Ok(())
}
