use super::*;
use du_core_contracts::job::{
    HistoryAppend, HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobHistory,
    JobOutcome, JobState, JobSummary, ProgressUnit, ResourceClass,
};
use du_job_runtime::{
    CancelResult, JobClock, JobEvent, JobEvents, JobPriority, JobRuntime, Submission, SubmitResult,
};
use std::sync::{Barrier, Mutex};
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        0
    }
}
struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}
struct Events;
impl JobEvents for Events {
    fn publish(&self, _: JobEvent) {}
}
struct Body;
impl JobBody for Body {
    fn kind_id(&self) -> &str {
        "viz.compute"
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
fn committed_eviction_stays_alive_until_f24_gate_releases() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let session = Arc::new(Session::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
        bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
    ));
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let first = session
        .render_selected(
            request(&parent, 1000),
            |_, item, _| Ok(policy(item)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let old = Arc::downgrade(&first.model);
    drop(first);
    let candidate = session
        .prepare_selected(
            request(&parent, 2000),
            |_, item, _| Ok(policy(item)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let deferred = Arc::new(Mutex::new(
        session.defer_reserved(session.reserve_prepared(candidate)),
    ));
    let runtime = Arc::new(
        JobRuntime::new(
            Arc::new(Identity(AtomicU8::new(40))),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events),
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    runtime
        .register(JobDescriptor {
            kind_id: "viz.compute".into(),
            version: 1,
            display_name_key: "viz.compute".into(),
            controls: vec![JobControl::Cancel],
            safe_points: vec!["segment".into()],
            progress_unit: ProgressUnit::Items,
            resource_classes: vec![ResourceClass::Compute],
            history: HistoryMode::None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let held = deferred.clone();
    let s = session.clone();
    let e = entered.clone();
    let r = release.clone();
    let submitted = runtime
        .submit_viz_with_publication(
            Submission {
                kind_id: "viz.compute".into(),
                deduplication_key: None,
                body: Arc::new(Body),
                priority: JobPriority::Normal,
                scope_summary: "test".into(),
                correlation_id: id(6),
                sample: false,
            },
            move || {
                let mut transaction = held.lock().map_err(|_| "lock".to_owned())?;
                s.commit_deferred(&mut transaction, &mut || false)
                    .map_err(|e| format!("{e:?}"))?;
                e.wait();
                r.wait();
                Ok(())
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let SubmitResult::Created(job) = submitted else {
        return Err("unexpected reuse".into());
    };
    let worker = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.run_next())
    };
    entered.wait();
    assert!(old.upgrade().is_some());
    let cancel = {
        let runtime = runtime.clone();
        let job = job.clone();
        std::thread::spawn(move || runtime.cancel(&job))
    };
    release.wait();
    worker
        .join()
        .map_err(|_| "worker panic")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        cancel
            .join()
            .map_err(|_| "cancel panic")?
            .map_err(|e| format!("{e:?}"))?,
        CancelResult::AlreadyTerminal(JobState::Done)
    );
    assert!(old.upgrade().is_some());
    drop(deferred);
    assert!(old.upgrade().is_none());
    Ok(())
}
