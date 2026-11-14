//! Live bounded scheduler state for one metadata scan.
use super::*;
use preparation::Prepared;

/// Builds the shared queue from prepared roots.
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
    let root_count = roots.len();
    if !devices.valid(root_count) || starts.len() != root_count {
        return Err(BuildError::Unavailable);
    }
    let boundaries: Vec<_> = starts
        .iter()
        .map(|start| match start {
            admission::RootStart::Collect(boundary) => Some(*boundary),
            admission::RootStart::Revisit => None,
        })
        .collect();
    let outstanding = if control.is_some() {
        let mut paths = queue::OutstandingPaths::new(root_count);
        for (index, _) in boundaries.iter().enumerate().filter(|(_, b)| b.is_some()) {
            if !paths.register(index as u32, &[]) {
                return Err(BuildError::Unavailable);
            }
        }
        Some(paths)
    } else {
        None
    };
    let ready: VecDeque<Frame> = roots
        .into_iter()
        .zip(&boundaries)
        .enumerate()
        .filter_map(|(index, (root, boundary))| Some(root_frame(index, root, (*boundary)?)))
        .collect();
    let shared = Shared {
        queue: Arc::new(Mutex::new(Queue {
            ready_high_water: diagnostics.map(|_| ready.len()),
            remaining: ready.len(),
            ready,
            root_remaining: boundaries
                .iter()
                .map(|b| usize::from(b.is_some()))
                .collect(),
            root_groups: devices.groups,
            active_groups: vec![0; root_count],
            device_limit: devices.limit,
            last_root: None,
            priority: None,
            outstanding,
            started: false,
        })),
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

fn root_frame(index: usize, directory: Box<dyn AnchoredDirectory>, boundary: Boundary) -> Frame {
    Frame {
        root_index: index as u32,
        path: Vec::new(),
        directory,
        entries: VecDeque::new(),
        deferred: VecDeque::new(),
        root: true,
        first_batch: false,
        root_published: false,
        batch_pending: false,
        case_observed: false,
        boundary,
    }
}
