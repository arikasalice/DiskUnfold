//! F02 FR10.HD1.app-host.DD1: each lane reports what it waits for.
use super::*;
use du_core_contracts::job::{LaneReport, LaneStatusReport, LaneWait};
use std::sync::mpsc;

type Gate = Arc<(Mutex<bool>, Condvar)>;

/// A root whose first read signals and then blocks until its gate opens.
struct BlockedRoot {
    entered: Option<mpsc::Sender<()>>,
    gate: Option<Gate>,
    done: bool,
}

fn io_error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.error".into(),
        os_code: None,
    }
}

impl DirectoryObservations for BlockedRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if std::mem::replace(&mut self.done, true) {
            return Ok(Vec::new());
        }
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
        }
        if let Some(gate) = &self.gate {
            let (lock, wake) = &**gate;
            let mut open = lock.lock().map_err(|_| io_error())?;
            while !*open {
                open = wake.wait(open).map_err(|_| io_error())?;
            }
        }
        Ok(Vec::new())
    }
}

impl AnchoredDirectory for BlockedRoot {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(io_error())
    }
}

/// A test context that keeps every lane report.
struct Reporting {
    inner: Context,
    reports: Mutex<Vec<LaneStatusReport>>,
}

impl JobContext for Reporting {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Continue
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        self.inner.try_start_flow()
    }
    fn report_lane_status(&self, status: LaneStatusReport) {
        if let Ok(mut reports) = self.reports.lock() {
            reports.push(status);
        }
    }
}

impl Reporting {
    fn latest(&self) -> Option<LaneStatusReport> {
        self.reports.lock().ok()?.last().cloned()
    }
}

fn lane(report: &LaneStatusReport, index: usize) -> Option<&LaneReport> {
    report.lanes.get(index)
}

fn assert_priority_lane(
    report: &LaneStatusReport,
    index: usize,
    key: Option<&str>,
    remaining: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let lane = lane(report, index).ok_or("missing lane")?;
    assert_eq!(lane.priority_area.as_deref(), key);
    assert_eq!(lane.remaining_prioritized_directories.as_deref(), remaining);
    Ok(())
}

#[test]
fn a_lane_blocked_in_an_os_call_reports_a_confirmed_io_wait_and_ends_idle()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let build = sink.open_forest(
        ids.next_id(),
        vec![observed_root("C:\\A", 1), observed_root("D:\\B", 2)],
    );
    let (entered, blocked) = mpsc::channel();
    let gate: Gate = Arc::new((Mutex::new(false), Condvar::new()));
    let dirs: Vec<Box<dyn AnchoredDirectory>> = vec![
        Box::new(BlockedRoot {
            entered: Some(entered),
            gate: Some(gate.clone()),
            done: false,
        }),
        Box::new(BlockedRoot {
            entered: None,
            gate: None,
            done: false,
        }),
    ];
    let context = Reporting {
        inner: Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        },
        reports: Mutex::new(Vec::new()),
    };
    let (during, outcome) = std::thread::scope(|scope| {
        let task = scope.spawn(|| {
            collect_open_forest_with_groups(
                build,
                dirs,
                &ids,
                &context,
                2,
                &|_| true,
                DevicePlan {
                    groups: vec![0, 1],
                    limit: 1,
                },
            )
        });
        let _ = blocked.recv_timeout(Duration::from_secs(2));
        // Wait for a sample taken while lane 0 is inside its read and lane 1 is done.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut during = None;
        while during.is_none() && std::time::Instant::now() < deadline {
            during = context.latest().filter(|report| {
                lane(report, 0).is_some_and(|lane| lane.in_flight == 1)
                    && lane(report, 1).is_some_and(|lane| lane.remaining_directories == 0)
            });
            std::thread::sleep(Duration::from_millis(10));
        }
        let (lock, wake) = &*gate;
        if let Ok(mut open) = lock.lock() {
            *open = true;
            wake.notify_all();
        }
        (during, task.join())
    });
    assert!(
        outcome
            .map_err(|_| "worker panic")?
            .result_version_id
            .is_some()
    );
    let during = during.ok_or("no sample while lane 0 was blocked")?;
    let blocked_lane = lane(&during, 0).ok_or("lane 0")?;
    assert_eq!(blocked_lane.root_indexes, [0]);
    assert_eq!(blocked_lane.limit, 1);
    assert_eq!(
        (blocked_lane.wait, blocked_lane.wait_confirmed),
        (LaneWait::IoWait, true)
    );
    let idle = lane(&during, 1).ok_or("lane 1")?;
    assert_eq!(idle.root_indexes, [1]);
    assert_eq!((idle.wait, idle.wait_confirmed), (LaneWait::None, true));
    // The last report, sent after the collection ended, shows both lanes idle.
    let last = context.latest().ok_or("no last report")?;
    for lane in &last.lanes {
        assert_eq!((lane.in_flight, lane.remaining_directories), (0, 0));
        assert_eq!((lane.wait, lane.wait_confirmed), (LaneWait::None, true));
    }
    Ok(())
}

#[test]
fn priority_status_follows_its_lane_replacement_completion_and_job_end()
-> Result<(), Box<dyn std::error::Error>> {
    let mut outstanding = queue::OutstandingPaths::new(2);
    let area_a = vec![b"A".to_vec()];
    let area_b = vec![b"B".to_vec()];
    let child_b = vec![b"B".to_vec(), b"child".to_vec()];
    assert!(outstanding.register(0, &area_a));
    assert!(outstanding.register(0, &area_b));
    assert!(outstanding.register(0, &child_b));
    let mut queue = Queue::for_test(vec![0, 1], 1);
    queue.outstanding = Some(outstanding);
    let shared = Shared::for_test(queue, true);
    let session = Arc::new(ScanControlSession::new(2));
    session.attach(&shared.queue, &shared.wake);
    let control = session.handle();

    assert_eq!(
        control.prioritize(0, area_a.clone(), "key-a".into()),
        Ok(crate::scan::ScanPriorityOutcome::Accepted {
            remaining_directories: 1
        })
    );
    let first = shared.lane_report().ok_or("no initial lane report")?;
    assert_priority_lane(&first, 0, Some("key-a"), Some("1"))?;
    assert_priority_lane(&first, 1, None, None)?;

    assert_eq!(
        control.prioritize(0, area_b.clone(), "key-b".into()),
        Ok(crate::scan::ScanPriorityOutcome::Accepted {
            remaining_directories: 2
        })
    );
    let replaced = shared.lane_report().ok_or("no replacement lane report")?;
    assert_priority_lane(&replaced, 0, Some("key-b"), Some("2"))?;

    {
        let mut queue = shared.queue.lock().map_err(|_| "queue lock poisoned")?;
        assert!(queue.complete_path(0, &area_b));
    }
    let partial = shared.lane_report().ok_or("no partial lane report")?;
    assert_priority_lane(&partial, 0, Some("key-b"), Some("1"))?;
    {
        let mut queue = shared.queue.lock().map_err(|_| "queue lock poisoned")?;
        assert!(queue.complete_path(0, &child_b));
    }
    let completed = shared.lane_report().ok_or("no completed lane report")?;
    assert_priority_lane(&completed, 0, None, None)?;

    let ending_area = vec![b"ending".to_vec()];
    {
        let mut queue = shared.queue.lock().map_err(|_| "queue lock poisoned")?;
        let outstanding = queue
            .outstanding
            .as_mut()
            .ok_or("priority path tracking is disabled")?;
        assert!(outstanding.register(0, &ending_area));
    }
    assert!(matches!(
        control.prioritize(0, ending_area, "key-at-end".into()),
        Ok(crate::scan::ScanPriorityOutcome::Accepted { .. })
    ));
    {
        let mut queue = shared.queue.lock().map_err(|_| "queue lock poisoned")?;
        queue.clear_priority();
    }
    session.finish();
    let ended = shared.lane_report().ok_or("no final lane report")?;
    assert_priority_lane(&ended, 0, None, None)?;
    Ok(())
}
