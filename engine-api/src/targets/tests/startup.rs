use super::*;
use du_core_contracts::volume::{
    Capability, CapabilityState, RootVolumeGroup, RootVolumeGroupObservation, VolumeCapabilities,
    VolumeCapacity, VolumeEnumeration, VolumeGroupRole, VolumeMount,
};
use std::sync::Arc;

struct StartupVolumes {
    mounts: Vec<VolumeMount>,
    group: RootVolumeGroupObservation,
}

impl VolumePort for StartupVolumes {
    fn enumerate(&self) -> VolumeEnumeration {
        let supported = Capability {
            state: CapabilityState::Supported,
            reason_key: None,
        };
        VolumeEnumeration {
            mounts: Ok(self.mounts.clone()),
            capabilities: VolumeCapabilities {
                enumerate: supported.clone(),
                identity: supported.clone(),
                capacity: supported.clone(),
                path_access: supported.clone(),
                path_compare_rule: supported.clone(),
                mount_boundary: supported.clone(),
                alias: supported,
            },
        }
    }

    fn root_volume_group(&self) -> RootVolumeGroupObservation {
        self.group.clone()
    }

    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        Ok(())
    }

    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        Ok(VolumeCapacity {
            total_bytes: 100,
            available_bytes: 50,
        })
    }
}

fn startup_mount(path: &[u8], role: Option<VolumeGroupRole>, name: Option<&str>) -> VolumeMount {
    let mount_path = if cfg!(windows) {
        let drive = match path {
            b"/" => b'C',
            b"/System/Volumes/Data" => b'D',
            _ => b'E',
        };
        vec![drive, 0, b':', 0, b'\\', 0]
    } else {
        path.to_vec()
    };
    VolumeMount {
        display_path: if cfg!(windows) {
            String::from_utf8_lossy(&mount_path).into_owned()
        } else {
            String::from_utf8_lossy(path).into_owned()
        },
        mount_path,
        name: name.map(str::to_owned),
        group_role: role,
        ..VolumeMount::default()
    }
}

fn confirmed_group() -> RootVolumeGroupObservation {
    RootVolumeGroupObservation::Confirmed(RootVolumeGroup {
        data_mount: b"/System/Volumes/Data".to_vec(),
        system_name: Some("Macintosh HD".into()),
        device: 1,
        data_inode: 2,
        firmlinks: Vec::new(),
    })
}

#[test]
fn target_list_marks_only_the_confirmed_system_mount_and_carries_optional_names()
-> Result<(), Box<dyn std::error::Error>> {
    let (_, workers) = setup()?;
    let volumes = Arc::new(StartupVolumes {
        mounts: vec![
            startup_mount(b"/", Some(VolumeGroupRole::System), Some("Macintosh HD")),
            startup_mount(b"/System/Volumes/Data", Some(VolumeGroupRole::Data), None),
            startup_mount(b"/Other", None, None),
        ],
        group: confirmed_group(),
    });
    let result = list_with_volumes(workers, None, volumes)
        .execute(TargetListRequest { refresh: false }, &caller())
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    assert_eq!(result.targets.len(), 3);
    let startup = result
        .targets
        .iter()
        .find(|target| target.volume_name.as_deref() == Some("Macintosh HD"))
        .ok_or("missing named startup target")?;
    assert_eq!(startup.is_startup_volume, Some(true));
    assert_eq!(
        result
            .targets
            .iter()
            .filter(|target| target.is_startup_volume == Some(true))
            .count(),
        1
    );
    assert!(
        result
            .targets
            .iter()
            .filter(|target| target.volume_name.is_none())
            .count()
            >= 2
    );
    Ok(())
}

#[test]
fn target_list_omits_startup_marker_without_a_confirmed_group()
-> Result<(), Box<dyn std::error::Error>> {
    for group in [
        RootVolumeGroupObservation::Unconfirmed,
        RootVolumeGroupObservation::NotApplicable,
    ] {
        let (_, workers) = setup()?;
        let volumes = Arc::new(StartupVolumes {
            mounts: vec![startup_mount(
                b"/",
                Some(VolumeGroupRole::System),
                Some("Macintosh HD"),
            )],
            group,
        });
        let result = list_with_volumes(workers, None, volumes)
            .execute(TargetListRequest { refresh: false }, &caller())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        assert_eq!(result.targets[0].is_startup_volume, None);
        assert_eq!(
            result.targets[0].volume_name.as_deref(),
            Some("Macintosh HD")
        );
    }
    Ok(())
}

#[test]
fn target_list_rejects_multiple_startup_markers() -> Result<(), Box<dyn std::error::Error>> {
    let (_, workers) = setup()?;
    let volumes = Arc::new(StartupVolumes {
        mounts: vec![
            startup_mount(b"/", Some(VolumeGroupRole::System), Some("Macintosh HD")),
            startup_mount(b"/Other", Some(VolumeGroupRole::System), Some("Other")),
        ],
        group: confirmed_group(),
    });
    assert!(
        list_with_volumes(workers, None, volumes)
            .execute(TargetListRequest { refresh: false }, &caller())
            .is_err()
    );
    Ok(())
}
