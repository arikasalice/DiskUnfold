//! Reports the per-lane state of a running collection (F02 FR10.HD1.app-host.DD1).
//!
//! A reporter thread samples every lane once per batch interval and hands the
//! sample to the job context, which publishes the latest value per job. It
//! reports once more when the collection ends and then stops.
use super::{Shared, lane_meter::LaneSample};
use du_core_contracts::job::{JobContext, LaneReport, LaneStatusReport};
use std::{
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::Duration,
};

/// How often the lanes are sampled; the runtime batches at the same interval.
const INTERVAL: Duration = Duration::from_millis(100);

impl Shared {
    /// One sample of every lane, or none when the queue lock is poisoned.
    pub(super) fn lane_report(&self) -> Option<LaneStatusReport> {
        let mut queue = self.queue.lock().ok()?;
        queue.recompute_resource_limits();
        let roots = queue.lanes.lane_roots();
        let lanes = roots
            .into_iter()
            .enumerate()
            .map(|(lane, root_indexes)| {
                let (members, limit, queued) = queue.lanes.occupancy(lane);
                let (device_cap_waiters, in_flight, blocked_sends, failures) =
                    self.meters.counts(lane);
                let resource = queue.lanes.resource(lane);
                let priority = queue.priority_for_roots(&root_indexes);
                let (device_kind, device_kind_source) = queue.lanes.device_kind(lane);
                let (wait, wait_confirmed) = LaneSample {
                    device_cap_waiters,
                    in_flight,
                    blocked_sends,
                    members,
                    limit,
                    queued,
                }
                .wait();
                LaneReport {
                    lane_index: u32::try_from(lane).unwrap_or(u32::MAX),
                    root_indexes,
                    in_flight,
                    limit: u32::try_from(limit).unwrap_or(u32::MAX).max(1),
                    device_kind,
                    device_kind_source,
                    mode: resource.map(|policy| policy.mode),
                    permit: resource.map(|policy| policy.permit),
                    share: resource.and_then(|policy| policy.share),
                    strategy: resource.map(|policy| policy.strategy.clone()),
                    priority_area: priority.map(|(area, _)| area.clone()),
                    remaining_prioritized_directories: priority.map(|(_, count)| count.to_string()),
                    wait,
                    wait_confirmed,
                    remaining_directories: u64::try_from(queued.saturating_add(members))
                        .unwrap_or(u64::MAX),
                    failures,
                }
            })
            .collect();
        Some(LaneStatusReport { lanes })
    }
}

/// Starts the reporter; dropping the returned sender makes it report a last
/// time and stop. A reporter that cannot start leaves the scan unchanged.
pub(super) fn spawn_reporter<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    shared: &'scope Shared,
    context: &'scope dyn JobContext,
) -> mpsc::Sender<()> {
    let (stop, stopped) = mpsc::channel::<()>();
    let _ = thread::Builder::new()
        .name("diskunfold-lane-status".into())
        .spawn_scoped(scope, move || {
            loop {
                let last = !matches!(
                    stopped.recv_timeout(INTERVAL),
                    Err(RecvTimeoutError::Timeout)
                );
                if let Some(report) = shared.lane_report() {
                    context.report_lane_status(report);
                }
                if last {
                    break;
                }
            }
        });
    stop
}
