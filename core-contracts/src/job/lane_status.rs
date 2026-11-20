//! Per-lane scan state reported through the job context (F02 FR10.HD1, port
//! side of HLD IF-17). It is not serialized; the desktop relay converts it to
//! the `scan.lane_status` wire body.

/// What one lane waits for, as the scan measured it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneWait {
    /// An OS call of the lane has not returned.
    IoWait,
    /// A worker of the lane waits for room in the observation channel.
    Backpressure,
    /// The lane has queued directories but all its worker seats are taken.
    DeviceCap,
    /// Nothing the lane is known to wait for.
    None,
}

/// The state of one storage lane of a scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneReport {
    /// Fixed for the lifetime of the job.
    pub lane_index: u32,
    /// The scope roots this lane serves, in ascending order.
    pub root_indexes: Vec<u32>,
    /// OS calls of the lane in flight.
    pub in_flight: u32,
    /// The lane's effective worker limit, at least 1.
    pub limit: u32,
    /// Requested scan resource mode, omitted when unavailable.
    pub mode: Option<crate::analysis_ipc::ScanResourceMode>,
    /// Device worker permit, omitted when unknown.
    pub permit: Option<u32>,
    /// Strategy identity used to calculate `limit`, omitted when unavailable.
    pub strategy: Option<crate::analysis_ipc::ScanLaneStrategy>,
    pub wait: LaneWait,
    /// Whether evidence confirms `wait`; when false `wait` is not used.
    pub wait_confirmed: bool,
    /// Directories known to the lane and not read to their end yet.
    pub remaining_directories: u64,
    /// Failures recorded for the lane's roots so far.
    pub failures: u64,
}

/// The latest lane states of one job. A later report replaces an earlier one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaneStatusReport {
    pub lanes: Vec<LaneReport>,
}
