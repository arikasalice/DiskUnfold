//! Live bounded scheduler state for one metadata scan.
use super::*;

pub(super) fn create(
    roots: Vec<Box<dyn AnchoredDirectory>>,
    devices: DevicePlan,
    diagnostics: Option<&crate::scan_job::ScanDiagnostics>,
    disconnects: disconnect::Disconnects,
    control: Option<&ScanControlSession>,
) -> Result<Shared, BuildError> {
    let root_count = roots.len();
    if !devices.valid(root_count) {
        return Err(BuildError::Unavailable);
    }
    let outstanding = if control.is_some() {
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
    let shared = Shared {
        queue: Arc::new(Mutex::new(Queue {
            ready_high_water: diagnostics.map(|_| root_count),
            ready: roots
                .into_iter()
                .enumerate()
                .map(|(index, root)| Frame {
                    root_index: index as u32,
                    path: Vec::new(),
                    directory: root,
                    entries: VecDeque::new(),
                    deferred: VecDeque::new(),
                    root: true,
                    first_batch: false,
                    root_published: false,
                    batch_pending: false,
                    case_observed: false,
                })
                .collect(),
            remaining: root_count,
            root_remaining: vec![1; root_count],
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
    };
    if let Some(control) = control {
        control.attach(&shared.queue, &shared.wake);
    }
    Ok(shared)
}
