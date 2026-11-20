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
