#![cfg(target_os = "macos")]
//! macOS volume adapter checks against the live mount table and isolated fixtures.
use du_core_contracts::{
    ports::PortErrorKind,
    scan_observation::ScopeRootPort,
    volume::{CapabilityState, VolumePort, VolumeRole},
};
use du_platform_os::{macos_observation::MacScopeRoots, volume::MacVolumes};
use std::{
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    },
    path::{Path, PathBuf},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A temporary directory addressed by its resolved path, since /var is a link.
fn fixture() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = fs::canonicalize(directory.path())?;
    Ok((directory, path))
}

fn raw(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

fn scope_identity(path: &Path) -> Result<Vec<u8>, String> {
    MacScopeRoots
        .open_root(raw(path))
        .map(|root| root.volume_id)
        .map_err(|e| format!("{e:?}"))
}

/// The mount path `statfs` reports for the volume holding `path`.
fn mount_of(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let stats = rustix::fs::statfs(path)?;
    Ok(stats
        .f_mntonname
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| u8::from_ne_bytes(c.to_ne_bytes()))
        .collect())
}

#[test]
fn listed_identities_match_scope_roots_on_the_same_volume() -> TestResult {
    let (_guard, path) = fixture()?;
    let listing = MacVolumes.enumerate();
    assert_eq!(
        listing.capabilities.enumerate.state,
        CapabilityState::Supported
    );
    let mounts = listing.mounts.map_err(|e| format!("{e:?}"))?;
    let system = mounts
        .iter()
        .find(|mount| mount.mount_path == b"/")
        .ok_or("the root mount is not listed")?;
    assert_eq!(
        system.volume_id.as_deref(),
        Some(scope_identity(Path::new("/"))?.as_slice())
    );
    let mount_path = mount_of(&path)?;
    let holding = mounts
        .iter()
        .find(|mount| mount.mount_path == mount_path)
        .ok_or("the fixture volume is not listed")?;
    let listed = holding
        .volume_id
        .as_deref()
        .ok_or("the fixture volume has no identity")?;
    assert_eq!(listed, scope_identity(&path)?.as_slice());
    if let Some(devices) = mounts.iter().find(|mount| mount.filesystem == b"devfs") {
        assert_eq!(devices.role, Some(VolumeRole::VirtualFs));
    }
    assert!(mounts.iter().all(|mount| {
        mount.mount_path.starts_with(b"/")
            && mount.topology.is_none()
            && mount.target_kind.is_none()
    }));
    Ok(())
}

#[test]
fn observation_confirms_the_identity_before_reading_capacity() -> TestResult {
    let (_guard, path) = fixture()?;
    let identity = scope_identity(&path)?;
    let probe = MacVolumes
        .observe(raw(&path), Some(&identity), &|| true)
        .map_err(|e| format!("{e:?}"))?;
    let capacity = probe
        .capacity
        .ok_or("capacity was not read")?
        .map_err(|e| format!("{e:?}"))?;
    assert!(capacity.total_bytes > 0);
    assert!(capacity.available_bytes <= capacity.total_bytes);
    let skipped = MacVolumes
        .observe(raw(&path), None, &|| false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(skipped.capacity.is_none());
    let other = MacVolumes
        .observe(raw(&path), Some(b"uuid:other"), &|| true)
        .err()
        .ok_or("another volume was accepted")?;
    assert_eq!(other.kind, PortErrorKind::IdentityChanged);
    let direct = MacVolumes
        .capacity(raw(&path))
        .map_err(|e| format!("{e:?}"))?;
    assert!(direct.available_bytes <= direct.total_bytes);
    Ok(())
}

#[test]
fn access_refuses_links_files_missing_and_invalid_paths() -> TestResult {
    let (_guard, path) = fixture()?;
    assert!(MacVolumes.path_access(raw(&path)).is_ok());
    let link = path.join("link");
    symlink(&path, &link)?;
    let file = path.join("file");
    fs::write(&file, b"x")?;
    let missing = path.join("missing");
    let through_link = link.join("inner");
    let cases: [(&[u8], PortErrorKind, &str); 6] = [
        (raw(&link), PortErrorKind::Unsupported, "path.unresolvable"),
        (
            raw(&through_link),
            PortErrorKind::Unsupported,
            "path.unresolvable",
        ),
        (raw(&file), PortErrorKind::Unsupported, "path.unresolvable"),
        (raw(&missing), PortErrorKind::NotFound, "os.query_failed"),
        (b"relative", PortErrorKind::Unsupported, "path.unresolvable"),
        (
            b"/private/../private",
            PortErrorKind::Unsupported,
            "path.unresolvable",
        ),
    ];
    for (target, kind, reason) in cases {
        let error = MacVolumes
            .path_access(target)
            .err()
            .ok_or_else(|| format!("{} was accepted", String::from_utf8_lossy(target)))?;
        assert_eq!((error.kind, error.reason_key.as_str()), (kind, reason));
    }
    Ok(())
}

#[test]
fn access_requires_read_and_search_permission() -> TestResult {
    let (_guard, path) = fixture()?;
    let closed = path.join("closed");
    fs::create_dir_all(closed.join("child"))?;
    for mode in [0o600, 0o300] {
        fs::set_permissions(&closed, fs::Permissions::from_mode(mode))?;
        // A superuser passes both checks, so only an enforced denial is expected.
        let enforced =
            fs::metadata(closed.join("child")).is_err() || fs::read_dir(&closed).is_err();
        let result = MacVolumes.path_access(raw(&closed));
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o700))?;
        if enforced {
            assert_eq!(
                result.err().map(|e| e.kind),
                Some(PortErrorKind::PermissionDenied),
                "mode {mode:o}"
            );
        }
    }
    Ok(())
}
