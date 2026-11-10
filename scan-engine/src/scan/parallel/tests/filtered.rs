use super::*;
use crate::scan::collect_parallel_filtered;
use du_core_contracts::scan_observation::{FilterDecision, FilteredBatch};

struct FilteredRoot {
    step: u8,
    probes: Arc<AtomicUsize>,
}
impl DirectoryObservations for FilteredRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "unexpected.legacy".into(),
            os_code: None,
        })
    }
    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        self.step += 1;
        let name = match self.step {
            1 => b"skip".as_slice(),
            2 => b"keep".as_slice(),
            _ => return Ok(FilteredBatch::End),
        };
        match include(name) {
            FilterDecision::Exclude => return Ok(FilteredBatch::Progress),
            FilterDecision::Undecidable => return Ok(FilteredBatch::Undecidable),
            FilterDecision::Admit => {}
        }
        self.probes.fetch_add(1, Ordering::SeqCst);
        let mut source = SmallRoot {
            fail: false,
            done: false,
            name: name.to_vec(),
        };
        Ok(FilteredBatch::Entries(source.next_batch()?))
    }
}
impl AnchoredDirectory for FilteredRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}

#[test]
fn skipped_batch_is_not_eof_and_unsupported_fails_closed() -> Result<(), Box<dyn std::error::Error>>
{
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let sink = IngestSink::default();
    let probes = Arc::new(AtomicUsize::new(0));
    let root = FilteredRoot {
        step: 0,
        probes: probes.clone(),
    };
    let outcome = collect_parallel_filtered(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        &context,
        1,
        &|root, parent, name| {
            if root == 0 && parent.is_empty() && name != b"skip" {
                FilterDecision::Admit
            } else {
                FilterDecision::Exclude
            }
        },
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(
        result
            .items()
            .iter()
            .map(|item| item.name.as_slice())
            .collect::<Vec<_>>(),
        [b"".as_slice(), b"keep".as_slice()]
    );
    assert_eq!(probes.load(Ordering::SeqCst), 1);
    let other = IngestSink::default();
    let legacy = SmallRoot {
        fail: false,
        done: false,
        name: b"unsafe".to_vec(),
    };
    let outcome = collect_parallel_filtered(
        &other,
        ids.next_id(),
        Box::new(legacy),
        &ids,
        &context,
        1,
        &|_, _, _| FilterDecision::Admit,
    );
    assert!(outcome.result_version_id.is_none());
    assert!(other.pin().map_err(|e| format!("{e:?}"))?.is_none());
    Ok(())
}

struct PartialRoot {
    sink: IngestSink,
    baseline: Id,
    child: bool,
    step: u8,
    unsupported: bool,
    saw_partial: Arc<std::sync::atomic::AtomicBool>,
    probes: Arc<AtomicUsize>,
}

impl DirectoryObservations for PartialRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "unexpected.legacy".into(),
            os_code: None,
        })
    }
    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        if self.step > 0 {
            return Ok(FilteredBatch::End);
        }
        self.step += 1;
        if self.child {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            loop {
                if self
                    .sink
                    .pin()
                    .ok()
                    .flatten()
                    .is_some_and(|result| result.version_id() != &self.baseline)
                {
                    self.saw_partial.store(true, Ordering::SeqCst);
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    return Err(PortError {
                        kind: PortErrorKind::Io,
                        reason_key: "test.timeout".into(),
                        os_code: None,
                    });
                }
                std::thread::yield_now();
            }
            if self.unsupported {
                return Err(PortError {
                    kind: PortErrorKind::Unsupported,
                    reason_key: "os.unsupported".into(),
                    os_code: None,
                });
            }
            return match include(b"unknown") {
                FilterDecision::Undecidable => Ok(FilteredBatch::Undecidable),
                FilterDecision::Exclude => Ok(FilteredBatch::Progress),
                FilterDecision::Admit => {
                    self.probes.fetch_add(1, Ordering::SeqCst);
                    Ok(FilteredBatch::End)
                }
            };
        }
        if include(b"folder") != FilterDecision::Admit {
            return Ok(FilteredBatch::Undecidable);
        }
        let mut source = SmallRoot {
            fail: false,
            done: false,
            name: b"folder".to_vec(),
        };
        let mut entries = source.next_batch()?;
        entries[0].enumerated_kind = ObservedKind::Directory;
        if let Ok(meta) = &mut entries[0].metadata {
            meta.kind = ObservedKind::Directory;
        }
        Ok(FilteredBatch::Entries(entries))
    }
}

impl AnchoredDirectory for PartialRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Ok(Box::new(Self {
            sink: self.sink.clone(),
            baseline: self.baseline.clone(),
            child: true,
            step: 0,
            unsupported: self.unsupported,
            saw_partial: self.saw_partial.clone(),
            probes: self.probes.clone(),
        }))
    }
}

#[test]
fn undecidable_or_unsupported_after_partial_withdraws_the_publication()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    for unsupported in [false, true] {
        let ids = Ids(AtomicUsize::new(1));
        let context = Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        };
        let sink = IngestSink::default();
        let mut prior = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
        prior.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        let baseline = prior
            .seal(ids.next_id(), ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let saw_partial = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probes = Arc::new(AtomicUsize::new(0));
        let root = PartialRoot {
            sink: sink.clone(),
            baseline: baseline.version_id().clone(),
            child: false,
            step: 0,
            unsupported,
            saw_partial: saw_partial.clone(),
            probes: probes.clone(),
        };
        let outcome = collect_parallel_filtered(
            &sink,
            ids.next_id(),
            Box::new(root),
            &ids,
            &context,
            1,
            &|_, parent, _| {
                if parent.is_empty() {
                    FilterDecision::Admit
                } else {
                    FilterDecision::Undecidable
                }
            },
        );
        assert!(saw_partial.load(Ordering::SeqCst));
        assert!(outcome.result_version_id.is_none());
        assert_eq!(probes.load(Ordering::SeqCst), 0);
        assert_eq!(
            sink.pin()
                .map_err(|e| format!("{e:?}"))?
                .ok_or("missing baseline")?
                .version_id(),
            baseline.version_id()
        );
    }
    Ok(())
}
