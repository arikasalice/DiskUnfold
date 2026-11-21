#![cfg(target_os = "macos")]

use du_core_contracts::{
    scan_observation::{ObservedKind, ScopeRootPort},
    volume::{RootVolumeGroupObservation, SystemVolumeRole, VolumeGroupRole, VolumePort},
};
use du_platform_os::{macos_observation::MacScopeRoots, volume::MacVolumes};
use std::fs;

#[test]
fn confirms_root_group_firmlinks_and_mount_roles() -> Result<(), Box<dyn std::error::Error>> {
    let group = match MacVolumes.root_volume_group() {
        RootVolumeGroupObservation::Confirmed(group) => group,
        observation => {
            return Err(format!("root volume group was not confirmed: {observation:?}").into());
        }
    };
    let firmlink_file = fs::read("/usr/share/firmlinks")?;
    let line_count = firmlink_file
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty() && *line != b"\r")
        .count();
    assert_eq!(group.firmlinks.len(), line_count);
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
