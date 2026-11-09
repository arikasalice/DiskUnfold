use super::*;
use du_core_contracts::{
    job::JobFlowError,
    ports::{PortError, PortErrorKind},
    result::ResultVersionRead,
    scan_observation::{DirectoryObservations, MetadataObservation},
};
use std::sync::{Arc, atomic::AtomicUsize};

struct Ids(AtomicUsize);
impl ResultIds for Ids {
    fn next_id(&self) -> Id {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap_or_else(|_| unreachable!())
    }
}
struct Context {
    completed: Arc<AtomicUsize>,
    abandoned: Arc<AtomicUsize>,
}
struct Flow {
    completed: Arc<AtomicUsize>,
    abandoned: Arc<AtomicUsize>,
    finished: bool,
}
impl JobFlow for Flow {
    fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }
    fn finish(mut self: Box<Self>, _: JobCounts) {
        self.finished = true;
        self.completed.fetch_add(1, Ordering::SeqCst);
    }
}
impl Drop for Flow {
    fn drop(&mut self) {
        if !self.finished {
            self.abandoned.fetch_add(1, Ordering::SeqCst);
        }
    }
}
impl JobContext for Context {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Continue
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(Flow {
            completed: self.completed.clone(),
            abandoned: self.abandoned.clone(),
            finished: false,
        }))
    }
}
struct Root {
    complete: bool,
    reads: Arc<AtomicUsize>,
}
impl DirectoryObservations for Root {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        if std::mem::replace(&mut self.complete, true) {
            return Ok(Vec::new());
        }
        Ok((1..=8192)
            .map(|n| ObservedEntry {
                name: format!("f{n}").into_bytes(),
                enumerated_inode: n,
                enumerated_kind: ObservedKind::File,
                metadata: Ok(MetadataObservation {
                    kind: ObservedKind::File,
                    device: 1,
                    inode: n,
                    size: Ok(1),
                    allocated: None,
                    link_count: None,
                    space_identity: None,
                }),
            })
            .collect())
    }
}
impl AnchoredDirectory for Root {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}

#[test]
fn failed_second_spawn_closes_channels_and_finishes_all_flows()
-> Result<(), Box<dyn std::error::Error>> {
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let mut previous = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
    previous
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let baseline = previous
        .seal(ids.next_id(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    TEST_WORKER_STARTED.store(false, Ordering::Release);
    FAIL_SPAWN_AT.store(1, Ordering::Release);
    let reads = Arc::new(AtomicUsize::new(0));
    let result = collect_parallel(
        &sink,
        ids.next_id(),
        Box::new(Root {
            complete: false,
            reads: reads.clone(),
        }),
        &ids,
        &context,
        3,
        &|_| true,
    );
    FAIL_SPAWN_AT.store(usize::MAX, Ordering::Release);
    assert!(TEST_WORKER_STARTED.load(Ordering::Acquire));
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    assert!(result.result_version_id.is_none());
    assert_eq!(context.completed.load(Ordering::SeqCst), 3);
    assert_eq!(context.abandoned.load(Ordering::SeqCst), 0);
    assert_eq!(
        sink.pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing baseline")?
            .version_id(),
        baseline.version_id()
    );
    Ok(())
}
