#![cfg(target_os = "macos")]

use du_core_contracts::{
    scan_observation::{DirectoryObservations, ObservedEntry, ObservedKind, ScopeRootPort},
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

fn find_entry(
    directory: &mut dyn DirectoryObservations,
    name: &[u8],
) -> Result<ObservedEntry, String> {
    loop {
        let batch = directory
            .next_batch()
            .map_err(|error| format!("{error:?}"))?;
        if batch.is_empty() {
            return Err(format!("missing {}", String::from_utf8_lossy(name)));
        }
        if let Some(entry) = batch.into_iter().find(|entry| entry.name == name) {
            return Ok(entry);
        }
    }
}

/// #351: the volumes of one APFS container report their own usage, not the container's,
/// and the sealed system volume names the container of its disk.
#[test]
fn each_volume_of_the_container_reports_its_own_usage() -> Result<(), Box<dyn std::error::Error>> {
    let mounts = MacVolumes
        .enumerate()
        .mounts
        .map_err(|error| format!("mount enumeration failed: {error:?}"))?;
    let find = |path: &[u8]| {
        mounts
            .iter()
            .find(|mount| mount.mount_path == path)
            .ok_or("mount not listed")
    };
    let (system, data) = (find(b"/")?, find(b"/System/Volumes/Data")?);
    assert!(system.container.is_some());
    assert_eq!(system.container, data.container);
    let total = data
        .listed_capacity
        .as_ref()
        .ok_or("no listed capacity")?
        .total_bytes;
    let (system_used, data_used) = (
        system.used_bytes.ok_or("no system usage")?,
        data.used_bytes.ok_or("no data usage")?,
    );
    // Two volumes of one container use less than the container; the container's own
    // used blocks would be reported twice and could not.
    assert_ne!(system_used, data_used);
    assert!(
        system_used
            .checked_add(data_used)
            .is_some_and(|sum| sum < total)
    );
    Ok(())
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
    // The list differs between macOS releases; the group holds exactly the rows
    // present on both sides, and those are at least half of the listed rows.
    let listed = firmlink_file
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.strip_suffix(b"\r").unwrap_or(line).is_empty())
        .count();
    assert!(present_count > 0 && present_count * 2 >= listed);
    assert_eq!(group.firmlinks.len(), present_count);
    let users_firmlink = group
        .firmlinks
        .iter()
        .find(|entry| entry.source == b"/Users")
        .ok_or("missing /Users firmlink")?;
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
    let users = find_entry(&mut *root.directory, b"Users")?;
    assert_eq!(users.enumerated_kind, ObservedKind::Firmlink);
    let users_metadata = users
        .metadata
        .as_ref()
        .map_err(|error| format!("Users metadata failed: {error:?}"))?;
    assert_eq!(users_metadata.kind, ObservedKind::Firmlink);
    assert_eq!(
        (users_metadata.device, users_metadata.inode),
        (users_firmlink.device, users_firmlink.inode)
    );

    let mut volumes = MacScopeRoots
        .open_root(b"/System/Volumes")
        .map_err(|error| format!("opening volumes failed: {error:?}"))?;
    let data = find_entry(&mut *volumes.directory, b"Data")?;
    let data_metadata = data
        .metadata
        .as_ref()
        .map_err(|error| format!("Data metadata failed: {error:?}"))?;
    assert_eq!(data_metadata.kind, ObservedKind::MountPoint);
    assert_eq!(
        (data_metadata.device, data_metadata.inode),
        (group.device, group.data_inode)
    );
    Ok(())
}
