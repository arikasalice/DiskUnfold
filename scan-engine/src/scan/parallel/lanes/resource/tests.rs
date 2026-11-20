use super::super::LaneConfig;
use super::*;
use crate::scan::parallel::queue::LaneDevice;
use du_core_contracts::{
    analysis_ipc::ScanResourceMode,
    job::DiskSeat,
    scan_observation::{DeviceKind, DeviceKindSource},
};

fn lane(kind: DeviceKind, disk_key: &[u8]) -> LaneDevice {
    LaneDevice {
        kind,
        source: DeviceKindSource::Detected,
        disk_seat: DiskSeat::new(kind, disk_key.to_vec()),
    }
}

#[test]
fn shared_hdd_shares_are_weighted_by_remaining_directories_and_recomputed() {
    let mut lanes = Lanes::new(LaneConfig {
        groups: &[0, 1, 2, 3],
        limit: 4,
        capacity: 8,
        track_peak: false,
        lane_devices: Some(vec![
            lane(DeviceKind::Hdd, &[1]),
            lane(DeviceKind::Hdd, &[1]),
            lane(DeviceKind::Hdd, &[1]),
            lane(DeviceKind::Ssd, &[2]),
        ]),
    });
    lanes.apply_resource(
        Condition {
            mode: ScanResourceMode::Auto,
            user_limit: None,
        },
        &[6, 2, 0, 0],
    );
    assert_eq!(
        lanes
            .resource(0)
            .and_then(|policy| policy.share)
            .map(NonZeroU32::get),
        Some(3)
    );
    assert_eq!(
        lanes
            .resource(1)
            .and_then(|policy| policy.share)
            .map(NonZeroU32::get),
        Some(1)
    );
    assert_eq!(
        lanes
            .resource(2)
            .and_then(|policy| policy.share)
            .map(NonZeroU32::get),
        Some(1)
    );
    assert_eq!(lanes.resource(3).map(|policy| policy.share), Some(None));
    assert_eq!(
        lanes.resource(0).map(|policy| policy.effective_limit),
        Some(2)
    );

    lanes.recompute_resources(&[1, 3, 0, 0]);
    assert_eq!(
        lanes
            .resource(0)
            .and_then(|policy| policy.share)
            .map(NonZeroU32::get),
        Some(1)
    );
    assert_eq!(
        lanes
            .resource(1)
            .and_then(|policy| policy.share)
            .map(NonZeroU32::get),
        Some(3)
    );
    assert_eq!(
        lanes
            .resource(2)
            .and_then(|policy| policy.share)
            .map(NonZeroU32::get),
        Some(1)
    );
}
