use super::*;
use du_core_contracts::job::{
    HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobOutcome, ProgressUnit,
    ResourceClass,
};
use du_job_runtime::{JobPriority, Submission, SubmitResult};
use std::sync::{
    Barrier,
    atomic::{AtomicBool, Ordering},
};

struct BlockHistory {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    first: AtomicBool,
}
impl JobHistory for BlockHistory {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        if self.first.swap(false, Ordering::SeqCst) {
            self.entered.wait();
            self.release.wait();
        }
        Ok(HistoryAppend::Added)
    }
}
struct CancelledBody;
impl JobBody for CancelledBody {
    fn kind_id(&self) -> &str {
        "diagnostic.backlog"
    }
    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        JobOutcome {
            counts: Default::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

#[test]
fn delivery_backlog_returns_busy_from_registered_browse_open()
-> Result<(), Box<dyn std::error::Error>> {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Ids),
        Arc::new(Clock),
        Arc::new(BlockHistory {
            entered: entered.clone(),
            release: release.clone(),
            first: AtomicBool::new(true),
        }),
        Arc::new(DesktopEvents::default()),
    )?);
    runtime.register(JobDescriptor {
        kind_id: "diagnostic.backlog".into(),
        version: 1,
        display_name_key: "diagnostic".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::MetadataIo],
        history: HistoryMode::Summary,
    })?;
    for i in 0..64 {
        let job = match runtime.submit(Submission {
            kind_id: "diagnostic.backlog".into(),
            deduplication_key: None,
            body: Arc::new(CancelledBody),
            priority: JobPriority::Normal,
            scope_summary: "diagnostic".into(),
            correlation_id: Ids.next_id(),
            sample: false,
        })? {
            SubmitResult::Created(id) => id,
            _ => return Err("unexpected dedup".into()),
        };
        runtime.cancel(&job)?;
        if i == 0 {
            entered.wait();
        }
    }
    let probe = runtime.submit(Submission {
        kind_id: "diagnostic.backlog".into(),
        deduplication_key: None,
        body: Arc::new(CancelledBody),
        priority: JobPriority::Normal,
        scope_summary: "diagnostic".into(),
        correlation_id: Ids.next_id(),
        sample: false,
    });
    assert!(matches!(probe, Err(RuntimeError::DeliveryBusy)));
    let store = IngestSink::default();
    let mut scan = store.open(Ids.next_id()).map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for i in 0..SYNC_WORK {
        scan.add_directory(vec![format!("folder-{i:05}").into_bytes()])
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = Ids.next_id();
    scan.seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let workers = Arc::new(Workers::start(runtime.clone())?);
    let mut builder = EngineBuilder::default();
    register(
        &mut builder,
        Arc::new(ResultSets::new(store)),
        Arc::new(Ids),
        workers,
        &Entropy,
    )?;
    let engine = builder.build()?;
    let region_ref = region(&engine);
    let reply = call(
        &engine,
        "browse.open",
        json!({"regionRef":region_ref,"viewKind":"list","requestSeq":1,"resultVersion":version}),
        Channel::Gui,
    );
    release.wait();
    assert_eq!(
        reply.error.ok_or("expected error")?.reason_key,
        "query.busy"
    );
    Ok(())
}
