use super::{Frame, LaneDevice, Origin};

/// One item claimed from a lane for one worker turn.
pub(in crate::scan::parallel) struct Taken {
    pub(in crate::scan::parallel) frame: Frame,
    pub(in crate::scan::parallel) lane: usize,
    pub(in crate::scan::parallel) origin: Origin,
}

pub(in crate::scan::parallel) struct LaneConfig<'a> {
    pub(in crate::scan::parallel) groups: &'a [usize],
    pub(in crate::scan::parallel) limit: usize,
    pub(in crate::scan::parallel) capacity: usize,
    pub(in crate::scan::parallel) track_peak: bool,
    pub(in crate::scan::parallel) lane_devices: Option<Vec<LaneDevice>>,
}
