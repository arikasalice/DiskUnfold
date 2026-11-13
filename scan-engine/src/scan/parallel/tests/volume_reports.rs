use super::*;
use du_core_contracts::scan_observation::{VolumeSpace, VolumeSpaceReader};

struct Reader {
    calls: Arc<AtomicUsize>,
    fail_at: Option<usize>,
}
impl VolumeSpaceReader for Reader {
    fn read(&self, _: &[u8]) -> Result<VolumeSpace, PortError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_at == Some(n) {
            return Err(PortError {
                kind: PortErrorKind::Io,
                reason_key: "injected".into(),
                os_code: Some(5),
            });
        }
        Ok(VolumeSpace {
            total_bytes: 100,
            used_bytes: 50 + n as u64,
            free_bytes: 50 - n as u64,
        })
    }
}
struct VolumeRoot {
    calls: Arc<AtomicUsize>,
    fail_at: Option<usize>,
    done: bool,
}
impl DirectoryObservations for VolumeRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.done = true;
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for VolumeRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "unsupported".into(),
            os_code: None,
        })
    }
    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        Ok(Box::new(Reader {
            calls: self.calls.clone(),
            fail_at: self.fail_at,
        }))
    }
}

struct TimedContext {
    clock: AtomicUsize,
}
impl JobContext for TimedContext {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Continue
    }
    fn utc_now(&self) -> Option<du_core_contracts::Timestamp> {
        du_core_contracts::Timestamp::from_unix_millis(
            self.clock.fetch_add(1, Ordering::SeqCst) as i64
        )
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(Flow {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
            finished: false,
        }))
    }
}

#[test]
fn same_volume_is_observed_once_per_phase_and_failure_is_preserved()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let observed = vec![
        observed_root("C:\\A", 1),
        observed_root("C:\\B", 1),
        observed_root("D:\\C", 2),
    ];
    let first = Arc::new(AtomicUsize::new(0));
    let duplicate = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let dirs = vec![
        (first.clone(), Some(0)),
        (duplicate.clone(), None),
        (second.clone(), Some(1)),
    ]
    .into_iter()
    .map(|(calls, fail_at)| {
        Box::new(VolumeRoot {
            calls,
            fail_at,
            done: false,
        }) as Box<dyn AnchoredDirectory>
    })
    .collect();
    let context = TimedContext {
        clock: AtomicUsize::new(1),
    };
    let result = collect_open_forest(
        sink.open_forest(ids.next_id(), observed),
        dirs,
        &ids,
        &context,
        1,
        &|_| true,
    );
    assert!(result.completed_scope);
    let version = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    let reports = version.volume_reports();
    assert_eq!(reports.len(), 2);
    let started = reports[0]
        .start
        .as_ref()
        .and_then(|point| point.observed_at.as_ref())
        .ok_or("missing start time")?;
    let ended = reports[0]
        .end
        .as_ref()
        .and_then(|point| point.observed_at.as_ref())
        .ok_or("missing end time")?;
    assert_ne!(started, ended);
    assert_eq!(first.load(Ordering::SeqCst), 2);
    assert_eq!(duplicate.load(Ordering::SeqCst), 0);
    assert_eq!(second.load(Ordering::SeqCst), 2);
    let error = reports[0]
        .start
        .as_ref()
        .ok_or("missing start")?
        .value
        .as_ref()
        .err()
        .ok_or("expected failure")?;
    assert_eq!(error.reason_key, "space.volume_report_unavailable");
    assert_eq!(error.os_code, Some(5));
    assert!(reports[0].end.as_ref().ok_or("missing end")?.value.is_ok());
    assert!(
        reports[1]
            .start
            .as_ref()
            .ok_or("missing start")?
            .value
            .is_ok()
    );
    assert!(reports[1].end.as_ref().ok_or("missing end")?.value.is_err());
    Ok(())
}

struct CancelRoot {
    calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}
impl DirectoryObservations for CancelRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.stop.store(true, Ordering::SeqCst);
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for CancelRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "unsupported".into(),
            os_code: None,
        })
    }
    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        Ok(Box::new(Reader {
            calls: self.calls.clone(),
            fail_at: None,
        }))
    }
}
struct CancelContext {
    stop: Arc<AtomicBool>,
}
impl JobContext for CancelContext {
    fn checkpoint(&self) -> Checkpoint {
        if self.stop.load(Ordering::SeqCst) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn utc_now(&self) -> Option<du_core_contracts::Timestamp> {
        du_core_contracts::Timestamp::parse("2020-09-13T12:26:40.000Z").ok()
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(Flow {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
            finished: false,
        }))
    }
}
#[test]
fn cancellation_keeps_timed_start_and_does_not_attempt_end()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let stop = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let ctx = CancelContext { stop: stop.clone() };
    let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(CancelRoot {
        calls: calls.clone(),
        stop,
    })];
    let result = collect_open_forest(
        sink.open_forest(ids.next_id(), vec![observed_root("C:\\A", 1)]),
        roots,
        &ids,
        &ctx,
        1,
        &|_| true,
    );
    assert!(!result.completed_scope);
    let version = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(version.completeness(), ResultCompleteness::Cancelled);
    let report = &version.volume_reports()[0];
    assert!(
        report
            .start
            .as_ref()
            .ok_or("missing start")?
            .observed_at
            .is_some()
    );
    assert!(report.end.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn unsupported_root_is_recorded_without_an_end_attempt() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(SmallRoot {
        fail: false,
        done: false,
        name: native("f"),
    })];
    let outcome = collect_open_forest(
        sink.open_forest(ids.next_id(), vec![observed_root("C:\\A", 1)]),
        roots,
        &ids,
        &context,
        1,
        &|_| true,
    );
    assert!(outcome.completed_scope);
    let version = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    let report = &version.volume_reports()[0];
    assert_eq!(
        report
            .start
            .as_ref()
            .ok_or("missing start")?
            .value
            .as_ref()
            .err()
            .map(|e| e.kind.clone()),
        Some(PortErrorKind::Unsupported)
    );
    assert!(report.end.is_none());
    Ok(())
}

struct EndCancelReader {
    calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    cancel_on_end: bool,
}
impl VolumeSpaceReader for EndCancelReader {
    fn read(&self, _: &[u8]) -> Result<VolumeSpace, PortError> {
        let prior = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.cancel_on_end && prior == 1 {
            self.stop.store(true, Ordering::SeqCst);
        }
        Ok(VolumeSpace {
            total_bytes: 100,
            used_bytes: 40,
            free_bytes: 60,
        })
    }
}
struct EndCancelRoot {
    calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    cancel_on_end: bool,
}
impl DirectoryObservations for EndCancelRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for EndCancelRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "unsupported".into(),
            os_code: None,
        })
    }
    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        Ok(Box::new(EndCancelReader {
            calls: self.calls.clone(),
            stop: self.stop.clone(),
            cancel_on_end: self.cancel_on_end,
        }))
    }
}
#[test]
fn cancellation_during_end_read_seals_cancelled_and_skips_remaining_volumes()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    for count in [1, 2] {
        let ids = Ids(AtomicUsize::new(1));
        let sink = IngestSink::default();
        let stop = Arc::new(AtomicBool::new(false));
        let calls: Vec<_> = (0..count).map(|_| Arc::new(AtomicUsize::new(0))).collect();
        let roots: Vec<Box<dyn AnchoredDirectory>> = calls
            .iter()
            .enumerate()
            .map(|(index, calls)| {
                Box::new(EndCancelRoot {
                    calls: calls.clone(),
                    stop: stop.clone(),
                    cancel_on_end: index == 0,
                }) as Box<dyn AnchoredDirectory>
            })
            .collect();
        let observed = (0..count)
            .map(|index| observed_root(&format!("C:\\R{index}"), index as u8 + 1))
            .collect();
        let ctx = CancelContext { stop };
        let outcome = collect_open_forest(
            sink.open_forest(ids.next_id(), observed),
            roots,
            &ids,
            &ctx,
            1,
            &|_| true,
        );
        let version = sink
            .pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing result")?;
        assert_eq!(version.completeness(), ResultCompleteness::Cancelled);
        assert!(!outcome.completed_scope);
        assert_eq!(
            outcome.result_version_id.as_ref(),
            Some(version.version_id())
        );
        assert_eq!(calls[0].load(Ordering::SeqCst), 2);
        assert!(version.volume_reports()[0].end.is_some());
        if count == 2 {
            assert_eq!(calls[1].load(Ordering::SeqCst), 1);
            assert!(version.volume_reports()[1].start.is_some());
            assert!(version.volume_reports()[1].end.is_none());
        }
    }
    Ok(())
}

struct CancelWhileWaiting {
    stop: Arc<AtomicBool>,
    calls: AtomicUsize,
}
impl JobContext for CancelWhileWaiting {
    fn checkpoint(&self) -> Checkpoint {
        if self.stop.load(Ordering::SeqCst) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) < 2 {
            Ok(Box::new(Flow {
                completed: Arc::new(AtomicUsize::new(0)),
                abandoned: Arc::new(AtomicUsize::new(0)),
                finished: false,
            }))
        } else {
            self.stop.store(true, Ordering::SeqCst);
            Err(JobFlowError::Busy)
        }
    }
}
#[test]
fn cancellation_while_waiting_for_end_flow_seals_cancelled()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let stop = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(EndCancelRoot {
        calls: calls.clone(),
        stop: stop.clone(),
        cancel_on_end: false,
    })];
    let ctx = CancelWhileWaiting {
        stop,
        calls: AtomicUsize::new(0),
    };
    let outcome = collect_open_forest(
        sink.open_forest(ids.next_id(), vec![observed_root("C:\\A", 1)]),
        roots,
        &ids,
        &ctx,
        1,
        &|_| true,
    );
    let version = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(version.completeness(), ResultCompleteness::Cancelled);
    assert!(!outcome.completed_scope);
    assert!(version.volume_reports()[0].end.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

mod disconnect;
