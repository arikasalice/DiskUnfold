//! The F02 `scan.lane_status` event body (HLD IF-08).
use crate::Timestamp;
use serde::{Deserialize, Serialize};

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
    /// Requested resource strategy; omitted when the producer cannot report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub mode: Option<ScanResourceMode>,
    /// Device worker permit; omitted when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub permit: Option<u32>,
    /// Strategy identity used to calculate `limit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub strategy: Option<ScanLaneStrategy>,
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
