#![cfg(target_os = "macos")]

use du_core_contracts::{
    scan_observation::{ObservedKind, ScopeRootPort},
    volume::{RootVolumeGroupObservation, SystemVolumeRole, VolumeGroupRole, VolumePort},
};
use du_platform_os::{macos_observation::MacScopeRoots, volume::MacVolumes};
use std::{
    ffi::OsStr,
    fs,
    io::{self, ErrorKind},
    os::unix::ffi::OsStrExt,
    path::Path,
};

fn directory_exists(path: &[u8]) -> io::Result<Option<()>> {
    match fs::symlink_metadata(Path::new(OsStr::from_bytes(path))) {
        Ok(metadata) if metadata.is_dir() => Ok(Some(())),
        Ok(_) => Err(io::Error::new(
            ErrorKind::InvalidData,
            "firmlink endpoint is not a directory",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn both_sides_present_count(contents: &[u8], data_mount: &[u8]) -> io::Result<usize> {
    let mut count = 0;
    for raw_line in contents.split(|&byte| byte == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if line.is_empty() {
            continue;
        }
        let separator = line
            .iter()
            .position(|&byte| byte == b'\t')
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "malformed firmlink row"))?;
        let source = &line[..separator];
        let relative = &line[separator + 1..];
        let target = [data_mount, b"/", relative].concat();
        let source_exists = directory_exists(source)?;
        let target_exists = directory_exists(&target)?;
        if source_exists.is_some() != target_exists.is_some() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "only one firmlink endpoint exists",
            ));
        }
        count += usize::from(source_exists.is_some());
    }
    Ok(count)
}

#[test]
fn confirms_root_group_firmlinks_and_mount_roles() -> Result<(), Box<dyn std::error::Error>> {
    let group = match MacVolumes.root_volume_group() {
        RootVolumeGroupObservation::Confirmed(group) => group,
        observation => {
            return Err(format!("root volume group was not confirmed: {observation:?}").into());
        }
    };
    let firmlink_file = fs::read("/usr/share/firmlinks")?;
    let present_count = both_sides_present_count(&firmlink_file, &group.data_mount)?;
    assert_eq!(present_count, 18);
    assert_eq!(group.firmlinks.len(), present_count);
    assert!(
        group
            .firmlinks
            .iter()
            .any(|entry| entry.source == b"/Users")
    );
    assert_eq!(group.data_mount, b"/System/Volumes/Data");

    let mounts = MacVolumes
        .enumerate()
        .mounts
        .map_err(|error| format!("mount enumeration failed: {error:?}"))?
        .into_iter()
        .map(|mount| (mount.mount_path.clone(), mount))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        mounts[b"/System/Volumes/Data".as_slice()].group_role,
        Some(VolumeGroupRole::Data)
    );
    assert_eq!(mounts[b"/System/Volumes/Data".as_slice()].system_role, None);
    assert_eq!(
        mounts[b"/System/Volumes/VM".as_slice()].system_role,
        Some(SystemVolumeRole::Vm)
    );

    let mut root = MacScopeRoots
        .open_root(b"/")
        .map_err(|error| format!("opening root failed: {error:?}"))?;
    loop {
        let batch = root
            .directory
            .next_batch()
            .map_err(|error| format!("root enumeration failed: {error:?}"))?;
        if batch.is_empty() {
            break;
        }
        if let Some(users) = batch.iter().find(|entry| entry.name == b"Users") {
            assert_eq!(users.enumerated_kind, ObservedKind::Firmlink);
            assert_eq!(
                users
                    .metadata
                    .as_ref()
                    .map_err(|error| format!("Users metadata failed: {error:?}"))?
                    .kind,
                ObservedKind::Firmlink
            );
            return Ok(());
        }
    }
    Err("the root enumeration did not contain Users".into())
}
