use super::*;

fn mount(path: &[u8], role: VolumeGroupRole, used: Option<u64>) -> VolumeMount {
    VolumeMount {
        mount_path: path.to_vec(),
        group_role: Some(role),
        used_bytes: used,
        volume_id: (role == VolumeGroupRole::System).then(|| b"system".to_vec()),
        container: Some(b"container".to_vec()),
        ..VolumeMount::default()
    }
}

#[test]
fn root_group_adds_system_and_data_and_sorts_other_volumes() -> Result<(), &'static str> {
    let system = mount(b"/", VolumeGroupRole::System, Some(100));
    let data = mount(b"/System/Volumes/Data", VolumeGroupRole::Data, Some(200));
    let zeta = VolumeMount {
        mount_path: b"/Volumes/Zeta".to_vec(),
        group_role: None,
        system_role: Some(SystemVolumeRole::Preboot),
        used_bytes: Some(20),
        container: Some(b"container".to_vec()),
        ..VolumeMount::default()
    };
    let alpha = VolumeMount {
        mount_path: b"/Volumes/Alpha".to_vec(),
        group_role: None,
        system_role: Some(SystemVolumeRole::Vm),
        used_bytes: Some(10),
        container: Some(b"container".to_vec()),
        ..VolumeMount::default()
    };
    let group = root_volume_group(true, Some(b"system"), None, &[system, zeta, data, alpha])
        .ok_or("expected root volume group")?;
    assert_eq!(group.used_bytes, Some(300));
    assert_eq!(group.other_volumes[0].volume_name, "Alpha");
    assert_eq!(group.other_volumes[0].role, "volume_role.vm");
    assert_eq!(group.other_volumes[1].volume_name, "Zeta");
    assert_eq!(group.other_volumes[1].role, "volume_role.preboot");
    assert_eq!(group.out_of_scope.exact_bytes(), Some(30));
    Ok(())
}

#[test]
fn missing_or_overflowing_group_usage_is_unknown() -> Result<(), &'static str> {
    let system = mount(b"/", VolumeGroupRole::System, None);
    let data = mount(b"/System/Volumes/Data", VolumeGroupRole::Data, Some(1));
    let missing = root_volume_group(true, Some(b"system"), None, &[system, data])
        .ok_or("expected group with missing usage")?;
    assert_eq!(missing.used_bytes, None);

    let system = mount(b"/", VolumeGroupRole::System, Some(u64::MAX));
    let data = mount(b"/System/Volumes/Data", VolumeGroupRole::Data, Some(1));
    let overflow = root_volume_group(true, Some(b"system"), None, &[system, data])
        .ok_or("expected group with overflowing usage")?;
    assert_eq!(overflow.used_bytes, None);
    Ok(())
}

#[test]
fn unknown_other_volume_makes_out_of_scope_unknown_and_non_root_is_omitted()
-> Result<(), &'static str> {
    let system = mount(b"/", VolumeGroupRole::System, Some(1));
    let data = mount(b"/System/Volumes/Data", VolumeGroupRole::Data, Some(1));
    let other = VolumeMount {
        mount_path: b"/Volumes/VM".to_vec(),
        system_role: Some(SystemVolumeRole::Vm),
        container: Some(b"container".to_vec()),
        ..VolumeMount::default()
    };
    let group = root_volume_group(true, Some(b"system"), None, &[system, data, other])
        .ok_or("expected root volume group")?;
    assert!(matches!(group.out_of_scope, FactorValue::Unknown { .. }));
    assert!(root_volume_group(false, Some(b"system"), None, &[]).is_none());
    Ok(())
}

#[test]
fn checked_other_volume_sum_rejects_overflow() -> Result<(), &'static str> {
    let system = mount(b"/", VolumeGroupRole::System, Some(1));
    let data = mount(b"/System/Volumes/Data", VolumeGroupRole::Data, Some(1));
    let first = VolumeMount {
        mount_path: b"/Volumes/A".to_vec(),
        system_role: Some(SystemVolumeRole::Vm),
        used_bytes: Some(u64::MAX),
        container: Some(b"container".to_vec()),
        ..VolumeMount::default()
    };
    let second = VolumeMount {
        mount_path: b"/Volumes/B".to_vec(),
        ..first.clone()
    };
    let group = root_volume_group(true, Some(b"system"), None, &[system, data, first, second])
        .ok_or("expected root volume group")?;
    assert!(matches!(group.out_of_scope, FactorValue::Unknown { .. }));
    Ok(())
}
