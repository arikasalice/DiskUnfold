//! Live bounded scheduler state for one metadata scan.
use super::*;
use preparation::Prepared;

/// Creates one lane per device group and queues every collected root in its lane.
///
/// A root recorded as a revisit of an earlier root gets no frame and no
/// outstanding work, so it is never enumerated or prioritized.
pub(super) fn create(
    prepared: Prepared,
    devices: DevicePlan,
    diagnostics: Option<&crate::scan_job::ScanDiagnostics>,
    disconnects: disconnect::Disconnects,
    control: Option<&ScanControlSession>,
) -> Result<Shared, BuildError> {
    let Prepared {
        roots,
        starts,
        visited,
    } = prepared;
    if starts.len() != roots.len() {
        return Err(BuildError::Unavailable);
    }
    let boundaries: Vec<_> = starts
        .iter()
        .map(|start| match start {
            admission::RootStart::Collect(boundary) => Some(*boundary),
            admission::RootStart::Revisit => None,
        })
        .collect();
    let options = LaneOptions {
        capacity: QUEUED_DIRECTORIES,
        track_peak: diagnostics.is_some(),
        track_paths: control.is_some(),
    };
    let queue = queue_for(roots, &boundaries, &devices, options)?;
    let shared = Shared {
        queue: Arc::new(Mutex::new(queue)),
        wake: Arc::new(Condvar::new()),
        abort: AtomicBool::new(false),
        disconnects,
        track_priority_paths: control.is_some(),
        visited,
    };
    if let Some(control) = control {
        control.attach(&shared.queue, &shared.wake);
    }
    Ok(shared)
}

/// Lane queue capacity and the optional accounting of one scheduler.
#[derive(Clone, Copy)]
pub(super) struct LaneOptions {
    /// Queued directory work per lane; see [`QUEUED_DIRECTORIES`].
    pub capacity: usize,
    /// Whether the peak of all lane queues is recorded for diagnostics.
    pub track_peak: bool,
    /// Whether outstanding directory paths are counted for priority areas.
    pub track_paths: bool,
}

/// Builds an unstarted queue; `boundaries[root]` is `None` for a revisited root.
pub(super) fn queue_for(
    roots: Vec<Box<dyn AnchoredDirectory>>,
    boundaries: &[Option<Boundary>],
    devices: &DevicePlan,
    options: LaneOptions,
) -> Result<Queue, BuildError> {
    let root_count = roots.len();
    if !devices.valid(root_count) || boundaries.len() != root_count || options.capacity == 0 {
        return Err(BuildError::Unavailable);
    }
    let mut outstanding = options
        .track_paths
        .then(|| queue::OutstandingPaths::new(root_count));
    let mut queue = Queue {
        lanes: lanes::Lanes::new(
            &devices.groups,
            devices.limit,
            options.capacity,
            options.track_peak,
        ),
        priority: None,
        outstanding: None,
        remaining: boundaries.iter().flatten().count(),
        root_remaining: boundaries
            .iter()
            .map(|b| usize::from(b.is_some()))
            .collect(),
        started: false,
    };
    for (index, (root, boundary)) in roots.into_iter().zip(boundaries).enumerate() {
        let Some(boundary) = *boundary else {
            continue;
        };
        let index = u32::try_from(index).map_err(|_| BuildError::Unavailable)?;
        if let Some(paths) = outstanding.as_mut()
            && !paths.register(index, &[])
        {
            return Err(BuildError::Unavailable);
        }
        if !queue.push_root(Frame::root(index, root, boundary)) {
            return Err(BuildError::Unavailable);
        }
    }
    queue.outstanding = outstanding;
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
