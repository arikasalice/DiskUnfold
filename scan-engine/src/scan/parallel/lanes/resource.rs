//! Applies and reports the per-lane static resource policy.
use super::{Lane, Lanes};
use crate::scan::parallel::resource::{self, Condition, Policy};
use du_core_contracts::{
    analysis_ipc::ScanSetResourceLane, job::DiskSeat, scan_observation::DeviceKind,
};
use du_job_runtime::device_permit;
use std::{collections::HashMap, num::NonZeroU32};

impl Lanes {
    pub(in crate::scan::parallel) fn apply_resource(
        &mut self,
        condition: Condition,
        remaining: &[usize],
    ) {
        self.condition = Some(condition);
        self.recompute_resources(remaining);
    }

    pub(in crate::scan::parallel) fn recompute_resources(&mut self, remaining: &[usize]) {
        let Some(condition) = self.condition else {
            return;
        };
        let shared = shared_disks(&self.lanes);
        for index in 0..self.lanes.len() {
            let (kind, share) = {
                let lane = &self.lanes[index];
                let kind = lane
                    .device
                    .as_ref()
                    .map_or(DeviceKind::Unknown, |device| device.kind);
                let share = lane.device.as_ref().and_then(|device| {
                    shared
                        .get(device.disk_seat.key())
                        .filter(|members| members.len() > 1)
                        .and_then(|members| lane_share(index, &self.lanes, members, remaining))
                });
                (kind, share)
            };
            let policy = resource::resolve_or_fallback(condition, kind, share);
            let lane = &mut self.lanes[index];
            lane.limit = match usize::try_from(policy.effective_limit) {
                Ok(limit) if limit > 0 => limit,
                _ => 1,
            };
            lane.resource = Some(policy);
        }
    }

    pub(in crate::scan::parallel) fn resource(&self, lane: usize) -> Option<&Policy> {
        self.lanes.get(lane)?.resource.as_ref()
    }

    pub(in crate::scan::parallel) fn disk_seat(&self, lane: usize) -> Option<&DiskSeat> {
        self.lanes
            .get(lane)?
            .device
            .as_ref()
            .map(|device| &device.disk_seat)
    }

    pub(in crate::scan::parallel) fn device_kind(
        &self,
        lane: usize,
    ) -> (
        DeviceKind,
        du_core_contracts::scan_observation::DeviceKindSource,
    ) {
        self.lanes
            .get(lane)
            .and_then(|lane| lane.device.as_ref())
            .map_or(
                (
                    DeviceKind::Unknown,
                    du_core_contracts::scan_observation::DeviceKindSource::Unknown,
                ),
                |device| (device.kind, device.source),
            )
    }

    pub(in crate::scan::parallel) fn resource_rows(&self) -> Vec<ScanSetResourceLane> {
        self.lanes
            .iter()
            .enumerate()
            .map(|(lane_index, lane)| ScanSetResourceLane {
                lane_index: u32::try_from(lane_index).unwrap_or(u32::MAX),
                effective_limit: u32::try_from(lane.limit).unwrap_or(u32::MAX).max(1),
                permit: lane.resource.as_ref().map(|resource| resource.permit),
                share: lane.resource.as_ref().and_then(|resource| resource.share),
            })
            .collect()
    }
}

fn shared_disks(lanes: &[Lane]) -> HashMap<Vec<u8>, Vec<usize>> {
    let mut shared = HashMap::new();
    for (index, lane) in lanes.iter().enumerate() {
        if let Some(device) = &lane.device {
            shared
                .entry(device.disk_seat.key().to_vec())
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    shared
}

fn lane_share(
    lane_index: usize,
    lanes: &[Lane],
    members: &[usize],
    remaining: &[usize],
) -> Option<NonZeroU32> {
    let permit = members
        .iter()
        .filter_map(|&index| lanes.get(index)?.device.as_ref())
        .map(|device| device_permit(device.kind))
        .min()?;
    let active_total = members
        .iter()
        .filter_map(|&index| remaining.get(index))
        .filter(|&&count| count > 0)
        .map(|&count| count as u128)
        .sum::<u128>();
    let lane_remaining = remaining.get(lane_index).copied().unwrap_or(0);
    let value = if lane_remaining == 0 || active_total == 0 {
        1
    } else {
        ((u128::from(permit) * lane_remaining as u128) / active_total)
            .max(1)
            .min(u128::from(u32::MAX)) as u32
    };
    NonZeroU32::new(value)
}

#[cfg(test)]
mod tests;
