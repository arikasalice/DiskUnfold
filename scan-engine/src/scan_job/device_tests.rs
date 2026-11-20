use super::*;
use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    ports::{PortError, PortErrorKind},
    scan_observation::{AnchoredDirectory, DirectoryObservations, ObservedEntry},
};

struct Empty;
impl DirectoryObservations for Empty {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for Empty {
    // Device grouping never collects, so identity is not supported here.
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        None
    }
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}
fn root(volume: &[u8], physical: Option<&[u8]>) -> (OpenedScopeRoot, RootDevice) {
    (
        OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&[65, 0]),
                display: "A".into(),
                lossy: false,
            },
            volume_id: volume.to_vec(),
            file_id: vec![1],
            directory: Box::new(Empty),
        },
        RootDevice {
            kind: DeviceKind::Ssd,
            source: DeviceKindSource::Detected,
            disk_key: physical.map(<[u8]>::to_vec),
        },
    )
}
#[test]
fn groups_by_volume_and_assigns_shared_disk_keys() {
    let roots = [
        root(&[1], Some(&[9])),
        root(&[1], None),
        root(&[2], Some(&[9])),
        root(&[3], None),
        root(&[], None),
        root(&[], None),
    ];
    let volumes: Vec<_> = roots
        .iter()
        .map(|(root, device)| (root.volume_id.clone(), device.clone()))
        .collect();
    let (groups, lanes) = lane_devices(&volumes);
    assert_eq!(groups[0], groups[1]);
    assert_ne!(groups[1], groups[2]);
    assert_ne!(groups[2], groups[3]);
    assert_eq!(groups[4], groups[5]);
    assert_ne!(groups[3], groups[4]);
    assert_eq!(lanes[0].disk_seat.key(), lanes[1].disk_seat.key());
}

#[test]
fn lane_kind_and_source_follow_root_consensus() {
    let mut roots = vec![root(&[1], Some(&[9])), root(&[1], Some(&[9]))];
    let volumes = |roots: &[(OpenedScopeRoot, RootDevice)]| {
        roots
            .iter()
            .map(|(root, device)| (root.volume_id.clone(), device.clone()))
            .collect::<Vec<_>>()
    };
    let (_, lanes) = lane_devices(&volumes(&roots));
    assert_eq!(
        lanes.first().map(|lane| (lane.kind, lane.source)),
        Some((DeviceKind::Ssd, DeviceKindSource::Detected))
    );

    if let Some((_, device)) = roots.get_mut(1) {
        device.kind = DeviceKind::Hdd;
    }
    let (_, lanes) = lane_devices(&volumes(&roots));
    assert_eq!(
        lanes.first().map(|lane| (lane.kind, lane.source)),
        Some((DeviceKind::Unknown, DeviceKindSource::Unknown))
    );
    assert_eq!(MetadataScan::max_device_permit(&volumes(&roots)), 16);

    if let Some((_, device)) = roots.get_mut(1) {
        device.kind = DeviceKind::Ssd;
        device.source = DeviceKindSource::User;
    }
    let (_, lanes) = lane_devices(&volumes(&roots));
    assert_eq!(
        lanes.first().map(|lane| (lane.kind, lane.source)),
        Some((DeviceKind::Ssd, DeviceKindSource::Unknown))
    );
}

#[test]
fn maximum_permit_uses_the_greatest_volume_lane() {
    let mut hdd_roots = vec![root(&[1], None), root(&[1], None)];
    for (_, device) in &mut hdd_roots {
        device.kind = DeviceKind::Hdd;
    }
    let hdd_volumes: Vec<_> = hdd_roots
        .iter()
        .map(|(root, device)| (root.volume_id.clone(), device.clone()))
        .collect();
    assert_eq!(MetadataScan::max_device_permit(&hdd_volumes), 4);

    let mut mixed_roots = [root(&[1], None), root(&[2], None)];
    if let Some((_, device)) = mixed_roots.get_mut(1) {
        device.kind = DeviceKind::Hdd;
    }
    let mixed_volumes: Vec<_> = mixed_roots
        .iter()
        .map(|(root, device)| (root.volume_id.clone(), device.clone()))
        .collect();
    assert_eq!(MetadataScan::max_device_permit(&mixed_volumes), 64);

    let mut unknown_roots = [root(&[1], None)];
    if let Some((_, device)) = unknown_roots.first_mut() {
        device.kind = DeviceKind::Unknown;
    }
    let unknown_volumes: Vec<_> = unknown_roots
        .iter()
        .map(|(root, device)| (root.volume_id.clone(), device.clone()))
        .collect();
    assert_eq!(MetadataScan::max_device_permit(&unknown_volumes), 16);
}
