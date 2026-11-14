//! Live bounded scheduler state for one metadata scan.
use super::*;

/// Creates one lane per device group and queues every root in its lane.
pub(super) fn create(
    roots: Vec<Box<dyn AnchoredDirectory>>,
    devices: DevicePlan,
    diagnostics: Option<&crate::scan_job::ScanDiagnostics>,
    disconnects: disconnect::Disconnects,
    control: Option<&ScanControlSession>,
) -> Result<Shared, BuildError> {
    let queue = queue_for(roots, &devices, QUEUED_DIRECTORIES, diagnostics.is_some(), control.is_some())?;
    let shared = Shared {
        queue: Arc::new(Mutex::new(queue)),
        wake: Arc::new(Condvar::new()),
        abort: AtomicBool::new(false),
        disconnects,
        track_priority_paths: control.is_some(),
        visited: Visited::default(),
    };
    if let Some(control) = control {
        control.attach(&shared.queue, &shared.wake);
    }
    Ok(shared)
}

/// Builds an unstarted queue with `capacity` queued items per lane.
pub(super) fn queue_for(
    roots: Vec<Box<dyn AnchoredDirectory>>,
    devices: &DevicePlan,
    capacity: usize,
    track_peak: bool,
    track_paths: bool,
) -> Result<Queue, BuildError> {
    let root_count = roots.len();
    if !devices.valid(root_count) || capacity == 0 {
        return Err(BuildError::Unavailable);
    }
    let outstanding = if track_paths {
        let mut paths = queue::OutstandingPaths::new(root_count);
        for index in 0..root_count {
            if !paths.register(index as u32, &[]) {
                return Err(BuildError::Unavailable);
            }
        }
        Some(paths)
    } else {
        None
    };
    let mut queue = Queue {
        lanes: lanes::Lanes::new(&devices.groups, devices.limit, capacity, track_peak),
        priority: None,
        outstanding,
        remaining: root_count,
        root_remaining: vec![1; root_count],
        started: false,
    };
    for (index, root) in roots.into_iter().enumerate() {
        if !queue.push_root(Frame::root(index as u32, root)) {
            return Err(BuildError::Unavailable);
        }
    }
    Ok(queue)
}

#[cfg(test)]
impl Queue {
    /// A started queue without roots; tests add frames and counts themselves.
    pub(super) fn for_test(groups: Vec<usize>, limit: usize) -> Self {
        let root_count = groups.len();
        Self {
            lanes: lanes::Lanes::new(&groups, limit, QUEUED_DIRECTORIES, true),
            priority: None,
            outstanding: None,
            remaining: 0,
            root_remaining: vec![0; root_count],
            started: true,
        }
    }
}

#[cfg(test)]
impl Shared {
    /// Wraps a test queue with default disconnection keys for its roots.
    pub(super) fn for_test(queue: Queue, track_priority_paths: bool) -> Self {
        let roots = queue.root_remaining.len();
        Self {
            queue: Arc::new(Mutex::new(queue)),
            wake: Arc::new(Condvar::new()),
            abort: AtomicBool::new(false),
            disconnects: disconnect::Disconnects::new(roots, &[]),
            track_priority_paths,
            visited: Visited::default(),
        }
    }
}
