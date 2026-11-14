use super::*;

struct DisconnectRoot {
    calls: Arc<AtomicUsize>,
    stage: u8,
}
impl DirectoryObservations for DisconnectRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if self.stage == 0 {
            self.stage = 1;
            return Ok(vec![ObservedEntry {
                name: native("observed.txt"),
                enumerated_inode: 7,
                enumerated_kind: ObservedKind::File,
                metadata: Err(PortError {
                    kind: PortErrorKind::Io,
                    reason_key: "scan.disconnected".into(),
                    os_code: Some(1167),
                }),
            }]);
        }
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "scan.disconnected".into(),
            os_code: Some(1167),
        })
    }
}
impl AnchoredDirectory for DisconnectRoot {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }
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

#[test]
fn disconnect_preserves_start_and_skips_only_affected_volume_end()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let first = Arc::new(AtomicUsize::new(0));
    let duplicate = Arc::new(AtomicUsize::new(0));
    let other = Arc::new(AtomicUsize::new(0));
    let roots = vec![
        observed_root("C:\\A", 1),
        observed_root("C:\\B", 1),
        observed_root("D:\\C", 2),
    ];
    let dirs: Vec<Box<dyn AnchoredDirectory>> = vec![
        Box::new(VolumeRoot {
            calls: first.clone(),
            fail_at: None,
            done: false,
        }),
        Box::new(DisconnectRoot {
            calls: duplicate.clone(),
            stage: 0,
        }),
        Box::new(VolumeRoot {
            calls: other.clone(),
            fail_at: None,
            done: false,
        }),
    ];
    let context = TimedContext {
        clock: AtomicUsize::new(1),
    };
    let result = collect_open_forest(
        sink.open_forest(ids.next_id(), roots),
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
    assert_eq!(first.load(Ordering::SeqCst), 1);
    assert_eq!(duplicate.load(Ordering::SeqCst), 0);
    assert_eq!(other.load(Ordering::SeqCst), 2);
    assert!(
        version.volume_reports()[0]
            .start
            .as_ref()
            .is_some_and(|p| p.value.is_ok())
    );
    assert!(version.volume_reports()[0].end.is_none());
    assert!(
        version.volume_reports()[1]
            .end
            .as_ref()
            .is_some_and(|p| p.value.is_ok())
    );
    Ok(())
}

struct StartDisconnectedReader(Arc<AtomicUsize>);
impl VolumeSpaceReader for StartDisconnectedReader {
    fn read(&self, _: &[u8]) -> Result<VolumeSpace, PortError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "scan.disconnected".into(),
            os_code: Some(1167),
        })
    }
}
struct StartDisconnectedRoot {
    reads: Arc<AtomicUsize>,
    enumerations: Arc<AtomicUsize>,
}
impl DirectoryObservations for StartDisconnectedRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.enumerations.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for StartDisconnectedRoot {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "unsupported".into(),
            os_code: None,
        })
    }
    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        Ok(Box::new(StartDisconnectedReader(self.reads.clone())))
    }
}
#[test]
fn initial_disconnection_is_retained_and_not_retried() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let enumerations = Arc::new(AtomicUsize::new(0));
    let roots = vec![observed_root("C:\\A", 1)];
    let dirs: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(StartDisconnectedRoot {
        reads: calls.clone(),
        enumerations: enumerations.clone(),
    })];
    let context = TimedContext {
        clock: AtomicUsize::new(1),
    };
    let result = collect_open_forest(
        sink.open_forest(ids.next_id(), roots),
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
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(enumerations.load(Ordering::SeqCst), 0);
    assert_eq!(version.completeness(), ResultCompleteness::Missing);
    assert!(
        version
            .scope_gaps()
            .iter()
            .any(|gap| gap.reason_key == "scan.disconnected")
    );
    let report = &version.volume_reports()[0];
    assert_eq!(
        report
            .start
            .as_ref()
            .and_then(|p| p.value.as_ref().err())
            .and_then(|e| e.os_code),
        Some(1167)
    );
    assert!(report.end.is_none());
    Ok(())
}
