//! The F02 `scan.lane_status` event body (HLD IF-08).
use crate::{
    ItemRef, Timestamp,
    scan_observation::{DeviceKind, DeviceKindSource},
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

use super::{ScanLaneStrategy, ScanResourceMode};

/// Topic of the batched per-lane scan state.
pub const SCAN_LANE_STATUS: &str = "scan.lane_status";
/// The most lanes one event carries.
pub const MAX_LANES: usize = 64;
/// The most root indexes one lane carries.
pub const MAX_LANE_ROOTS: usize = 64;

/// What one lane waits for. The spelling matches `AnalysisJobWaitReason`.
///
/// A receiver treats a value it does not know like an unconfirmed cause.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ScanLaneWait {
    IoWait,
    Backpressure,
    DeviceCap,
    None,
}

/// The state of one storage lane. The lane is named by its index and the
/// scope roots it serves; no storage key, path or name is carried.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanLane {
    /// Fixed for the lifetime of the job.
    pub lane_index: u32,
    /// The first [`MAX_LANE_ROOTS`] scope root indexes of the lane, ascending.
    pub root_indexes: Vec<u32>,
    /// All root indexes of the lane as a decimal string; present only when
    /// `root_indexes` was cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub root_count: Option<String>,
    /// OS calls of the lane in flight (u32).
    pub in_flight: u32,
    /// The lane's effective worker limit (u32, at least 1; 0 is never sent).
    pub limit: u32,
    /// Device class; omitted or unrecognized values mean `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "string"))]
    pub device_kind: Option<DeviceKind>,
    /// Classification source; omitted or unrecognized values mean `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "string"))]
    pub device_kind_source: Option<DeviceKindSource>,
    /// Requested resource strategy; omitted when the producer cannot report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub mode: Option<ScanResourceMode>,
    /// Device permit by kind; it keeps the full per-device permit when lanes share a disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub permit: Option<u32>,
    /// This lane's share of a permit split with other lanes on the same disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "number"))]
    pub share: Option<NonZeroU32>,
    /// Strategy identity used to calculate `limit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub strategy: Option<ScanLaneStrategy>,
    /// The same `ItemRef` the GUI sent to `scan.prioritize` and the engine
    /// returned as `appliedPriorityArea`; it carries no path or name and is
    /// omitted when this lane has no priority area. It remains present while
    /// the engine keeps the region, even at zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub priority_area: Option<ItemRef>,
    /// Known directories in `priority_area` not read to their end, decimal.
    /// Zero alone does not release the region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub remaining_prioritized_directories: Option<String>,
    /// The wait cause; not to be used when `wait_confirmed` is false.
    pub wait_reason: ScanLaneWait,
    pub wait_confirmed: bool,
    /// Directories known to the lane and not read to their end, decimal.
    pub remaining_directories: String,
    /// Failures recorded for the lane's roots so far, decimal.
    pub failures: String,
}

/// The latest lane states of one job, sent at most once per batch interval.
///
/// The envelope carries the job reference. `sequence` is taken from the
/// job's sequence space shared with `job.progress`, state changes and the
/// terminal event, so each topic sees gaps where the others were sent. A
/// receiver discards a value whose sequence is not greater than the last
/// lane status it applied for the job, and any value after the terminal event.
/// After `job.resync_required` it reads `job.status` and applies only later
/// values. Fields may be added as optional; receivers ignore unknown fields
/// and read an omitted field as unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScanLaneStatus {
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    /// The first [`MAX_LANES`] lanes by index.
    pub lanes: Vec<ScanLane>,
    /// All lanes as a decimal string; present only when `lanes` was cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub lane_count: Option<String>,
    /// UTC time at which the runtime observed this value.
    pub observed_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn lane(priority_area: Option<ItemRef>, remaining: Option<&str>) -> ScanLane {
        ScanLane {
            lane_index: 0,
            root_indexes: vec![0],
            root_count: None,
            in_flight: 0,
            limit: 1,
            mode: None,
            permit: None,
            strategy: None,
            priority_area,
            remaining_prioritized_directories: remaining.map(str::to_owned),
            wait_reason: ScanLaneWait::None,
            wait_confirmed: true,
            remaining_directories: "0".into(),
            failures: "0".into(),
        }
    }

    fn status(lane: ScanLane) -> Result<ScanLaneStatus, Box<dyn Error>> {
        Ok(ScanLaneStatus {
            sequence: 1,
            lanes: vec![lane],
            lane_count: None,
            observed_at: Timestamp::from_unix_millis(0).ok_or("invalid timestamp")?,
        })
    }

    #[test]
    fn lane_status_round_trips_priority_fields_verbatim_and_omits_them_when_absent()
    -> Result<(), Box<dyn Error>> {
        let item_ref = ItemRef {
            result_version: crate::Id::from_v7_bytes([1; 16]),
            item_no: 7,
        };
        let active = status(lane(Some(item_ref.clone()), Some("27")))?;
        let active_json = serde_json::to_value(&active)?;
        assert_eq!(
            active_json["lanes"][0]["priorityArea"],
            serde_json::to_value(&item_ref)?
        );
        assert_eq!(
            active_json["lanes"][0]["remainingPrioritizedDirectories"],
            "27"
        );
        assert_eq!(
            serde_json::from_value::<ScanLaneStatus>(active_json)?,
            active
        );

        let inactive = status(lane(None, None))?;
        let inactive_json = serde_json::to_value(&inactive)?;
        assert!(inactive_json["lanes"][0].get("priorityArea").is_none());
        assert!(
            inactive_json["lanes"][0]
                .get("remainingPrioritizedDirectories")
                .is_none()
        );
        assert_eq!(
            serde_json::from_value::<ScanLaneStatus>(inactive_json)?,
            inactive
        );
        Ok(())
    }
}
