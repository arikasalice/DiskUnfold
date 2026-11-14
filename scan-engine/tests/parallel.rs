use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError},
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, HandleIdentity, MetadataObservation,
        ObservedEntry, ObservedKind,
    },
};
use du_scan_engine::scan::{ResultIds, collect_parallel};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Ids(AtomicUsize);
impl ResultIds for Ids {
    fn next_id(&self) -> Id {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap_or_else(|_| unreachable!())
    }
}
struct Context {
    cancelled: AtomicBool,
    active: Arc<AtomicUsize>,
    busy: AtomicUsize,
    limit: usize,
}
impl JobContext for Context {
    fn checkpoint(&self) -> Checkpoint {
        if self.cancelled.load(Ordering::SeqCst) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        if self.limit == 0 {
            return Err(JobFlowError::UnsupportedResource);
        }
        self.active
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.limit).then_some(n + 1)
            })
            .map_err(|_| {
                self.busy.fetch_add(1, Ordering::SeqCst);
                JobFlowError::Busy
            })?;
        Ok(Box::new(Flow(self.active.clone())))
    }
}
struct Flow(Arc<AtomicUsize>);
impl Drop for Flow {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl JobFlow for Flow {
    fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }
    fn finish(self: Box<Self>, _: JobCounts) {}
}
struct Gate {
    released: Mutex<bool>,
    wake: Condvar,
    entered: AtomicUsize,
    opens: AtomicUsize,
}
struct Directory {
    name: Option<u8>,
    read: bool,
    gate: Arc<Gate>,
    delayed: bool,
}
fn error() -> PortError {
    PortError {
        kind: PortErrorKind::PermissionDenied,
        reason_key: "scan.path_denied".into(),
        os_code: None,
    }
}
fn entry(name: &[u8], kind: ObservedKind, inode: u64) -> ObservedEntry {
    ObservedEntry {
        name: name.to_vec(),
        enumerated_inode: inode,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device: 1,
            inode,
            size: Ok(3),
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
            native_modified_at: None,
        }),
    }
}
impl DirectoryObservations for Directory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if self.read {
            return Ok(Vec::new());
        }
        self.read = true;
        if let Some(name) = self.name {
            if self.delayed {
                self.gate.entered.fetch_add(1, Ordering::SeqCst);
                let mut released = self.gate.released.lock().map_err(|_| error())?;
                while !*released {
                    released = self.gate.wake.wait(released).map_err(|_| error())?;
                }
            }
            return Ok(vec![entry(&[name], ObservedKind::File, u64::from(name))]);
        }
        Ok(vec![
            entry(b"a", ObservedKind::Directory, 1),
            entry(b"b", ObservedKind::Directory, 2),
            entry(b"denied", ObservedKind::Directory, 3),
        ])
    }
}
impl AnchoredDirectory for Directory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        self.gate.opens.fetch_add(1, Ordering::SeqCst);
        if entry.name == b"denied" {
            return Err(error());
        }
        Ok(Box::new(Directory {
            name: entry.name.first().copied(),
            read: false,
            gate: self.gate.clone(),
            delayed: self.delayed,
        }))
    }
    fn observed_identity(&self) -> Option<HandleIdentity> {
        Some(root_identity())
    }
}
/// A root identity distinct from every fixture entry, on the same volume.
fn root_identity() -> HandleIdentity {
    HandleIdentity {
        device: 1,
        inode: u64::MAX,
        ancestors: Vec::new(),
    }
}
fn context(limit: usize) -> Context {
    Context {
        cancelled: AtomicBool::new(false),
        active: Arc::new(AtomicUsize::new(0)),
        busy: AtomicUsize::new(0),
        limit,
    }
}
fn gate() -> Arc<Gate> {
    Arc::new(Gate {
        released: Mutex::new(false),
        wake: Condvar::new(),
        entered: AtomicUsize::new(0),
        opens: AtomicUsize::new(0),
    })
}
fn root(gate: Arc<Gate>, delayed: bool) -> Box<dyn AnchoredDirectory> {
    Box::new(Directory {
        name: None,
        read: false,
        gate,
        delayed,
    })
}

#[test]
fn blocked_io_keeps_permits_and_cancellation_stops_new_opens()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ctx = context(2);
    let gate = gate();
    let ids = Ids(AtomicUsize::new(1));
    thread::scope(|scope| -> Result<(), Box<dyn std::error::Error>> {
        let run = scope.spawn(|| {
            collect_parallel(
                &sink,
                ids.next_id(),
                root(gate.clone(), true),
                &ids,
                &ctx,
                2,
                &|_| true,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while gate.entered.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            thread::yield_now();
        }
        if gate.entered.load(Ordering::SeqCst) < 2 {
            *gate.released.lock().map_err(|_| "poison")? = true;
            gate.wake.notify_all();
            return Err("two workers did not enter I/O".into());
        }
        while sink.pin().map_err(|e| format!("{e:?}"))?.is_none() && Instant::now() < deadline {
            thread::yield_now();
        }
        let partial = sink
            .pin()
            .map_err(|e| format!("{e:?}"))?
            .map(|v| (v.completeness(), v.items().len()));
        let busy = matches!(ctx.try_start_flow(), Err(JobFlowError::Busy));
        ctx.cancelled.store(true, Ordering::SeqCst);
        let opens = gate.opens.load(Ordering::SeqCst);
        *gate.released.lock().map_err(|_| "poison")? = true;
        gate.wake.notify_all();
        let outcome = run.join().map_err(|_| "worker panic")?;
        assert_eq!(partial, Some((ResultCompleteness::Partial, 4)));
        assert!(busy);
        assert_eq!(gate.opens.load(Ordering::SeqCst), opens);
        assert_eq!(ctx.active.load(Ordering::SeqCst), 0);
        assert!(!outcome.completed_scope);
        assert_eq!(
            sink.pin()
                .map_err(|e| format!("{e:?}"))?
                .ok_or("missing")?
                .completeness(),
            ResultCompleteness::Cancelled
        );
        Ok(())
    })
}

#[test]
fn worker_order_does_not_change_result_and_denial_is_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let ids = Ids(AtomicUsize::new(1));
    let mut snapshots = Vec::new();
    for limit in [1, 2, 4] {
        let sink = IngestSink::default();
        let ctx = context(limit);
        let outcome = collect_parallel(
            &sink,
            ids.next_id(),
            root(gate(), false),
            &ids,
            &ctx,
            limit,
            &|_| true,
        );
        assert_eq!(outcome.counts.failed, 1);
        let result = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
        assert_eq!(result.completeness(), ResultCompleteness::Missing);
        assert_eq!(result.items()[0].kind, ResultItemKind::Directory);
        snapshots.push(result.items().to_vec());
    }
    assert_eq!(snapshots[0], snapshots[1]);
    assert_eq!(snapshots[1], snapshots[2]);
    Ok(())
}

#[test]
fn busy_permit_waits_then_collects_after_release() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ctx = context(1);
    let gate = gate();
    let ids = Ids(AtomicUsize::new(1));
    let held = ctx.try_start_flow().map_err(|e| format!("{e:?}"))?;
    thread::scope(|scope| -> Result<(), Box<dyn std::error::Error>> {
        let run = scope.spawn(|| {
            collect_parallel(
                &sink,
                ids.next_id(),
                root(gate.clone(), false),
                &ids,
                &ctx,
                1,
                &|_| true,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while ctx.busy.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            thread::yield_now();
        }
        let waited = ctx.busy.load(Ordering::SeqCst) > 0;
        let opens_before_release = gate.opens.load(Ordering::SeqCst);
        held.finish(JobCounts::default());
        let outcome = run.join().map_err(|_| "worker panic")?;
        assert!(waited);
        assert_eq!(opens_before_release, 0);
        assert!(outcome.result_version_id.is_some());
        assert!(gate.opens.load(Ordering::SeqCst) > 0);
        Ok(())
    })
}

#[test]
fn busy_wait_can_be_cancelled_without_opening_children() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ctx = context(1);
    let gate = gate();
    let ids = Ids(AtomicUsize::new(1));
    let held = ctx.try_start_flow().map_err(|e| format!("{e:?}"))?;
    thread::scope(|scope| -> Result<(), Box<dyn std::error::Error>> {
        let run = scope.spawn(|| {
            collect_parallel(
                &sink,
                ids.next_id(),
                root(gate.clone(), false),
                &ids,
                &ctx,
                1,
                &|_| true,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while ctx.busy.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            thread::yield_now();
        }
        let waited = ctx.busy.load(Ordering::SeqCst) > 0;
        ctx.cancelled.store(true, Ordering::SeqCst);
        let outcome = run.join().map_err(|_| "worker panic")?;
        held.finish(JobCounts::default());
        assert!(waited);
        assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
        assert!(!outcome.completed_scope);
        assert_eq!(
            sink.pin()
                .map_err(|e| format!("{e:?}"))?
                .ok_or("missing")?
                .completeness(),
            ResultCompleteness::Cancelled
        );
        Ok(())
    })
}

#[test]
fn unavailable_flow_never_runs_os_calls() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ctx = context(0);
    let gate = gate();
    let ids = Ids(AtomicUsize::new(1));
    let outcome = collect_parallel(
        &sink,
        ids.next_id(),
        root(gate.clone(), false),
        &ids,
        &ctx,
        2,
        &|_| true,
    );
    assert!(outcome.result_version_id.is_none());
    assert_eq!(gate.opens.load(Ordering::SeqCst), 0);
    assert_eq!(gate.entered.load(Ordering::SeqCst), 0);
    assert!(sink.pin().map_err(|e| format!("{e:?}"))?.is_none());
    Ok(())
}

struct WideRoot {
    done: bool,
}
impl DirectoryObservations for WideRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if std::mem::replace(&mut self.done, true) {
            return Ok(Vec::new());
        }
        Ok((1..=200)
            .map(|n| entry(format!("d{n}").as_bytes(), ObservedKind::Directory, n))
            .collect())
    }
}
impl AnchoredDirectory for WideRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Ok(Box::new(WideRoot { done: true }))
    }
    fn observed_identity(&self) -> Option<HandleIdentity> {
        Some(root_identity())
    }
}

#[test]
fn full_directory_queue_finishes_without_losing_work() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let ctx = context(1);
    let ids = Ids(AtomicUsize::new(1));
    let outcome = collect_parallel(
        &sink,
        ids.next_id(),
        Box::new(WideRoot { done: false }),
        &ids,
        &ctx,
        1,
        &|_| true,
    );
    assert_eq!(outcome.counts.failed, 0);
    let result = sink.pin().map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    assert_eq!(result.completeness(), ResultCompleteness::Complete);
    assert_eq!(result.items().len(), 201);
    Ok(())
}

struct BatchRoot {
    count: u64,
    done: bool,
}
impl DirectoryObservations for BatchRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if std::mem::replace(&mut self.done, true) {
            return Ok(Vec::new());
        }
        Ok((1..=self.count)
            .map(|n| entry(format!("f{n}").as_bytes(), ObservedKind::File, n))
            .collect())
    }
}
impl AnchoredDirectory for BatchRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(error())
    }
}

#[test]
fn worker_batch_limit_is_distinct_from_the_stage_channel_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let ids = Ids(AtomicUsize::new(1));
    for count in [8192, 8193] {
        let sink = IngestSink::default();
        let ctx = context(1);
        let outcome = collect_parallel(
            &sink,
            ids.next_id(),
            Box::new(BatchRoot { count, done: false }),
            &ids,
            &ctx,
            1,
            &|_| true,
        );
        if count == 8192 {
            assert!(outcome.result_version_id.is_some());
            assert_eq!(
                sink.pin()
                    .map_err(|e| format!("{e:?}"))?
                    .ok_or("missing")?
                    .items()
                    .len(),
                8193
            );
        } else {
            assert!(outcome.result_version_id.is_none());
            assert!(sink.pin().map_err(|e| format!("{e:?}"))?.is_none());
        }
    }
    Ok(())
}
