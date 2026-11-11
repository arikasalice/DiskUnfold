//! Shared-ready deque scheduling and optional peak accounting.
use super::*;

impl Queue {
    pub(super) fn push_ready(&mut self, frame: Frame) {
        self.ready.push_back(frame);
        if let Some(peak) = &mut self.ready_high_water {
            *peak = (*peak).max(self.ready.len());
        }
    }

    pub(super) fn pop_fair(&mut self) -> Option<(Frame, usize)> {
        let last = self.last_root;
        let index = self
            .ready
            .iter()
            .enumerate()
            .filter(|(_, frame)| {
                self.active_groups[self.root_groups[frame.root_index as usize]] < self.device_limit
            })
            .min_by_key(|(_, frame)| {
                (
                    last.is_some_and(|value| frame.root_index <= value),
                    frame.root_index,
                )
            })
            .map(|(index, _)| index)?;
        let frame = self.ready.remove(index)?;
        self.last_root = Some(frame.root_index);
        let group = self.root_groups[frame.root_index as usize];
        self.active_groups[group] += 1;
        Some((frame, group))
    }
}

pub(super) fn record_ready_peak(
    shared: &Shared,
    diagnostics: Option<&crate::scan_job::ScanDiagnostics>,
) {
    if let Some(observer) = diagnostics
        && let Ok(queue) = shared.queue.lock()
        && let Some(peak) = queue.ready_high_water
    {
        observer.record_ready_peak(peak);
    }
}

pub(crate) struct DevicePlan {
    pub groups: Vec<usize>,
    pub limit: usize,
}
impl DevicePlan {
    pub(super) fn valid(&self, count: usize) -> bool {
        count != 0
            && u32::try_from(count).is_ok()
            && self.groups.len() == count
            && self.limit != 0
            && self.groups.iter().all(|&group| group < count)
    }
}
