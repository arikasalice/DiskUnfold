use super::*;
use crate::windows_observation::WindowsScopeRoots;
use du_core_contracts::scan_observation::ScopeRootPort;
use std::{fs, os::windows::ffi::OsStrExt};

#[test]
fn matching_scan_identity_is_held_and_replacement_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("root");
    fs::create_dir(&path)?;
    let native: Vec<u8> = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let scan = WindowsScopeRoots
        .open_root(&native)
        .map_err(|e| format!("{e:?}"))?;
    let canonical = Path::new(&scan.canonical_path.display);
    let root = open_matching_scan_root(canonical, &scan.volume_id, &scan.file_id)
        .map_err(|e| format!("{e:?}"))?;
    assert!(fs::rename(&path, fixture.path().join("old")).is_err());
    drop(root);
    fs::rename(&path, fixture.path().join("old"))?;
    fs::create_dir(&path)?;
    let verified = open_matching_scan_root(canonical, &scan.volume_id, &scan.file_id);
    assert!(matches!(
        verified,
        Err(PortError {
            kind: PortErrorKind::IdentityChanged,
            ..
        })
    ));
    Ok(())
}

#[test]
fn absent_or_unmatched_evidence_and_open_failure_do_not_continue()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("root");
    fs::create_dir(&path)?;
    let native: Vec<u8> = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let scan = WindowsScopeRoots
        .open_root(&native)
        .map_err(|e| format!("{e:?}"))?;
    for result in [
        open_matching_scan_root(&path, &[], &scan.file_id),
        open_matching_scan_root(&path, &scan.volume_id, &[]),
    ] {
        assert!(matches!(
            result,
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                ..
            })
        ));
    }
    assert!(matches!(
        open_matching_scan_root(&path, b"other-volume", &scan.file_id),
        Err(PortError {
            kind: PortErrorKind::IdentityChanged,
            ..
        })
    ));
    assert!(
        open_matching_scan_root(
            &fixture.path().join("missing"),
            &scan.volume_id,
            &scan.file_id
        )
        .is_err()
    );
    Ok(())
}
