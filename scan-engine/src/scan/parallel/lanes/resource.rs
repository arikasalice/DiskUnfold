//! Applies and reports a scan's static lane resource policy.
use super::Lanes;
use crate::scan::parallel::resource::{self, Condition, Policy};
use du_core_contracts::analysis_ipc::ScanSetResourceLane;

impl Lanes {
    pub(in crate::scan::parallel) fn apply_resource(&mut self, condition: Condition) {
        let policy = resource::resolve_or_fallback(condition, resource::UNKNOWN_DEVICE_PERMIT);
        let limit = match usize::try_from(policy.effective_limit) {
            Ok(limit) if limit != 0 => limit,
            _ => {
                eprintln!("warning: resource limit conversion failed; using one worker");
                1
            }
        };
        for lane in &mut self.lanes {
            lane.limit = limit;
            lane.resource = Some(policy.clone());
        }
    }

    pub(in crate::scan::parallel) fn resource(&self, lane: usize) -> Option<&Policy> {
        self.lanes.get(lane)?.resource.as_ref()
    }

    pub(in crate::scan::parallel) fn resource_rows(&self) -> Vec<ScanSetResourceLane> {
        self.lanes
            .iter()
            .enumerate()
            .map(|(lane_index, lane)| ScanSetResourceLane {
                lane_index: u32::try_from(lane_index).unwrap_or(u32::MAX),
                effective_limit: u32::try_from(lane.limit).unwrap_or(u32::MAX).max(1),
                permit: lane.resource.as_ref().map(|resource| resource.permit),
            })
            .collect()
    }
}
