use super::test_helpers::{finished_event, job_status, wait_terminal};
mod busy_repro;
use super::*;
use crate::{
    api::EngineBuilder,
    browse::folder::{FolderBrowse, job::FolderJobs},
    desktop::DesktopEvents,
    query_region::commands::Regions,
    workers::Workers,
};
use du_analysis_index::{IngestSink, Observation, query};
use du_core_contracts::{
    job::{HistoryAppend, JobHistory, JobSummary},
    ports::{PortError, Random},
    result::ResultCompleteness,
};
use du_job_runtime::{JobClock, JobRuntime};
use serde_json::json;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

struct Identity {
    next: AtomicUsize,
    cancel_on: AtomicUsize,
    replace_on: AtomicUsize,
    caller: Mutex<Option<CallerCtx>>,
    store: Mutex<Option<IngestSink>>,
}
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        let sequence = self.next.fetch_add(1, Ordering::SeqCst);
        if sequence == self.replace_on.load(Ordering::SeqCst)
            && let Ok(store) = self.store.lock()
            && let Some(store) = store.as_ref()
            && let Ok(mut scan) = store.open(id(81))
            && scan.add_directory(vec![]).is_ok()
            && scan
                .add_file(Observation {
                    path: vec![b"replacement".to_vec()],
                    size: None,
                })
                .is_ok()
        {
            let _ = scan.seal(id(82), ResultCompleteness::Complete);
        }
        if sequence == self.cancel_on.load(Ordering::SeqCst)
            && let Ok(caller) = self.caller.lock()
            && let Some(caller) = caller.as_ref()
        {
            caller.cancel();
        }
        let mut bytes = [0; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = u8::try_from(sequence).unwrap_or(0);
        Id::from_v7_bytes(bytes)
    }
}
struct Clock;
impl JobClock for Clock {
    fn monotonic_now(&self) -> Option<Duration> {
        Some(Duration::ZERO)
    }
    fn now_ms(&self) -> i64 {
        1
    }
}
struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}
struct Entropy;
impl Random for Entropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(9);
        Ok(())
    }
}
fn id(n: u8) -> Id {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = n;
    Id::from_v7_bytes(bytes)
}

#[test]
// This single trace deliberately covers ordering across source, region, and F24 gates.
#[allow(clippy::cognitive_complexity)]
fn invalid_cancelled_and_replaced_searches_preserve_published_results() -> Result<(), String> {
    let _serial = test_gate::serial().map_err(str::to_owned)?;
    let store = IngestSink::default();
    let mut scan = store.open(id(1)).map_err(|error| format!("{error:?}"))?;
    scan.add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    scan.add_file(Observation {
        path: vec![b"one".to_vec()],
        size: None,
    })
    .map_err(|error| format!("{error:?}"))?;
    scan.seal(id(2), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let sets = Arc::new(ResultSets::new(store));
    let old = query::run(
        &Registry::baseline(),
        sets.source()
            .pin()
            .map_err(|error| format!("{error:?}"))?
            .ok_or("no source")?,
        &Condition::All(vec![]),
        query::Sort {
            id: "sort.path",
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(|error| format!("{error:?}"))?;
    sets.register(id(80), old)
        .map_err(|error| format!("{error:?}"))?;

    let ids = Arc::new(Identity {
        next: AtomicUsize::new(0),
        cancel_on: AtomicUsize::new(usize::MAX),
        replace_on: AtomicUsize::new(usize::MAX),
        caller: Mutex::new(None),
        store: Mutex::new(Some(sets.source())),
    });
    let events = Arc::new(DesktopEvents::default());
    let runtime = Arc::new(
        JobRuntime::new(
            ids.clone(),
            Arc::new(Clock),
            Arc::new(History),
            events.clone(),
        )
        .map_err(|error| format!("{error:?}"))?,
    );
    let mut builder = EngineBuilder::default();
    let regions = Regions::register(&mut builder, ids.clone(), &Entropy, runtime.clone())
        .map_err(|error| format!("{error:?}"))?;
    crate::jobs::register(&mut builder, runtime.clone());
    let region_ref = regions
        .registry
        .open(&regions.owner, QueryRegionKind::Search)
        .map_err(|error| format!("{error:?}"))?;
    let caller = CallerCtx::new(Channel::Gui, id(4), None);
    let browse = Arc::new(FolderBrowse::with_result_sets(sets.clone(), ids.clone()));
    let jobs = Arc::new(FolderJobs::new(browse, runtime.clone()).map_err(|e| format!("{e:?}"))?);
    let workers = Arc::new(Workers::start(runtime.clone()).map_err(|e| format!("{e:?}"))?);
    let entry = SearchEntry::new(
        sets.clone(),
        regions,
        ids.clone(),
        super::SearchWorkers::new(jobs, workers),
    );
    let engine = builder.build().map_err(|error| error.to_string())?;
    let make_request =
        |request_seq, sort_id: &str, parent_search_ref, result_version, condition| SearchRequest {
            region_ref: region_ref.clone(),
            request_seq,
            result_version,
            parent_search_ref,
            condition,
            sorts: vec![SortSpec {
                id: sort_id.into(),
                revision: 1,
                basis: query::Basis::Logical,
                descending: false,
            }],
            count: None,
        };

    let first = entry
        .execute(
            make_request(1, "sort.path", None, Some(id(2)), Condition::All(vec![])),
            &caller,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(first.items().len(), 2);

    assert!(matches!(
        entry.execute(
            make_request(
                2,
                "sort.path",
                None,
                Some(id(2)),
                Condition::Predicate(query::Predicate::Registered {
                    id: "pred.missing".into(),
                    revision: 1,
                    operand: Vec::new(),
                }),
            ),
            &caller
        ),
        Err(SearchError::InvalidRequest)
    ));
    let old_page = sets
        .open_page(first.query_ref(), &id(2), 0, None)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(old_page.items().len(), 2);

    assert!(matches!(
        entry.execute(
            make_request(
                2,
                "sort.path",
                Some(first.query_ref().clone()),
                Some(id(82)),
                Condition::All(vec![])
            ),
            &caller
        ),
        Err(SearchError::StaleVersion)
    ));
    let refined = entry
        .execute(
            make_request(
                2,
                "sort.path",
                Some(first.query_ref().clone()),
                Some(id(2)),
                Condition::All(vec![]),
            ),
            &caller,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(refined.items().len(), 2);

    ids.cancel_on.store(5, Ordering::SeqCst);
    *ids.caller.lock().map_err(|_| "identity lock poisoned")? = Some(caller.clone());
    let outcome = entry.execute(
        make_request(3, "sort.path", None, Some(id(2)), Condition::All(vec![])),
        &caller,
    );
    assert!(matches!(outcome, Err(SearchError::Cancelled)));
    let old_page = sets
        .open_page(refined.query_ref(), &id(2), 0, None)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(old_page.items().len(), 2);

    ids.cancel_on.store(usize::MAX, Ordering::SeqCst);
    ids.replace_on.store(6, Ordering::SeqCst);
    let replacement_caller = CallerCtx::new(Channel::Gui, id(5), None);
    let replaced = entry
        .execute(
            make_request(4, "sort.path", None, Some(id(2)), Condition::All(vec![])),
            &replacement_caller,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(replaced.metadata().version, &id(2));
    assert_eq!(replaced.items().len(), 2);
    assert!(matches!(
        sets.open_page(replaced.query_ref(), &id(2), 0, None),
        Err(PageError::StaleVersion)
    ));
    assert_eq!(first.items().len(), 2);
    // Once admitted, abandoning the caller does not cancel or fail the F24 job.
    ids.replace_on.store(usize::MAX, Ordering::SeqCst);
    let caller = CallerCtx::new(Channel::Gui, id(6), None);
    let gate = test_gate::Gate::new(test_gate::Stage::Publish);
    test_gate::set(Some((id(82), gate.clone()))).map_err(str::to_owned)?;
    let admitted = entry
        .execute_async(
            make_request(5, "sort.path", None, Some(id(82)), Condition::All(vec![])),
            &caller,
        )
        .map_err(|error| format!("admitted async: {error:?}"))?;
    gate.entered().map_err(str::to_owned)?;
    let before_publish = runtime
        .snapshot(&admitted.job_ref)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        before_publish.state,
        du_core_contracts::job::JobState::Running
    );
    assert_eq!(before_publish.result_version_id, None);
    assert!(job_status(&engine, &admitted.job_ref)?["queryRef"].is_null());
    caller.cancel();
    gate.release().map_err(str::to_owned)?;
    let completed_snapshot = wait_terminal(&runtime, &admitted.job_ref)?;
    assert_eq!(
        completed_snapshot.state,
        du_core_contracts::job::JobState::Done
    );
    let query_ref = completed_snapshot
        .result_version_id
        .ok_or("missing admitted query")?;
    assert_eq!(
        finished_event(&events, &admitted.job_ref)?["body"]["queryRef"],
        json!(query_ref)
    );
    test_gate::set(None).map_err(str::to_owned)?;

    // A source replacement between evaluation and commit cannot create a page-only result.
    let gate = test_gate::Gate::new(test_gate::Stage::Publish);
    test_gate::set(Some((id(82), gate.clone()))).map_err(str::to_owned)?;
    let replaced_source = entry
        .execute_async(
            make_request(6, "sort.path", None, Some(id(82)), Condition::All(vec![])),
            &CallerCtx::new(Channel::Gui, id(7), None),
        )
        .map_err(|error| format!("source-replacement async: {error:?}"))?;
    gate.entered().map_err(str::to_owned)?;
    let mut replacement = sets.source().open(id(83)).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .add_file(Observation {
            path: vec![b"new-source".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(id(83), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    gate.release().map_err(str::to_owned)?;
    let stale_snapshot = wait_terminal(&runtime, &replaced_source.job_ref)?;
    assert_eq!(stale_snapshot.result_version_id, None);
    assert_eq!(
        stale_snapshot.state,
        du_core_contracts::job::JobState::Failed
    );
    assert_eq!(
        finished_event(&events, &replaced_source.job_ref)?["body"]["queryRef"],
        serde_json::Value::Null
    );
    test_gate::set(None).map_err(str::to_owned)?;

    // A successful F24 completion exposes the same query reference to job.get and job.finished.
    let completed = entry
        .execute_async(
            make_request(7, "sort.path", None, Some(id(83)), Condition::All(vec![])),
            &CallerCtx::new(Channel::Gui, id(8), None),
        )
        .map_err(|error| format!("successful async: {error:?}"))?;
    let done = wait_terminal(&runtime, &completed.job_ref)?;
    assert_eq!(done.state, du_core_contracts::job::JobState::Done);
    assert_eq!(
        done.terminal
            .as_ref()
            .map(|terminal| terminal.history_recorded),
        Some(false)
    );
    let query_ref = done
        .result_version_id
        .clone()
        .ok_or("queryRef was not attached")?;
    assert_eq!(
        job_status(&engine, &completed.job_ref)?["queryRef"],
        json!(query_ref)
    );
    let page = sets
        .open_page(&query_ref, &id(83), 0, None)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(page.items().len(), 2);
    let finished = finished_event(&events, &completed.job_ref)?;
    assert_eq!(finished["body"]["queryRef"], query_ref.as_str());

    // F24 cancellation after candidate evaluation but before its publication is authoritative.
    let gate = test_gate::Gate::new(test_gate::Stage::Candidate);
    test_gate::set(Some((id(83), gate.clone()))).map_err(str::to_owned)?;
    let cancelled_by_job = entry
        .execute_async(
            make_request(8, "sort.path", None, Some(id(83)), Condition::All(vec![])),
            &CallerCtx::new(Channel::Gui, id(9), None),
        )
        .map_err(|error| format!("F24 cancellation async: {error:?}"))?;
    gate.entered().map_err(str::to_owned)?;
    let cancel_caller = CallerCtx::new(Channel::Gui, id(92), None);
    let cancel = engine.call(
        du_core_contracts::CallEnvelope {
            name: "query.cancel".into(),
            contract_version: du_core_contracts::ENVELOPE_VERSION,
            correlation_id: cancel_caller.correlation_id.clone(),
            body: json!({
                "regionRef": region_ref,
                "target": {"kind": "job", "jobRef": cancelled_by_job.job_ref},
            }),
        },
        &cancel_caller,
    );
    assert!(cancel.error.is_none(), "{:?}", cancel.error);
    assert_eq!(cancel.body["outcome"], "accepted");
    gate.release().map_err(str::to_owned)?;
    let cancelled_snapshot = wait_terminal(&runtime, &cancelled_by_job.job_ref)?;
    assert_eq!(
        cancelled_snapshot.state,
        du_core_contracts::job::JobState::Cancelled
    );
    assert_eq!(cancelled_snapshot.result_version_id, None);
    assert_eq!(
        finished_event(&events, &cancelled_by_job.job_ref)?["body"]["queryRef"],
        serde_json::Value::Null
    );
    test_gate::set(None).map_err(str::to_owned)?;

    // An invalid new request is rejected before it can cancel the active region job.
    let gate = test_gate::Gate::new(test_gate::Stage::Worker);
    test_gate::set(Some((id(83), gate.clone()))).map_err(str::to_owned)?;
    let active = entry
        .execute_async(
            make_request(9, "sort.path", None, Some(id(83)), Condition::All(vec![])),
            &CallerCtx::new(Channel::Gui, id(10), None),
        )
        .map_err(|error| format!("active async: {error:?}"))?;
    gate.entered().map_err(str::to_owned)?;
    assert!(matches!(
        entry.execute_async(
            make_request(
                10,
                "sort.unknown",
                None,
                Some(id(83)),
                Condition::All(vec![])
            ),
            &CallerCtx::new(Channel::Gui, id(11), None),
        ),
        Err(SearchError::InvalidRequest)
    ));
    assert!(matches!(
        entry.execute_async(
            make_request(
                10,
                "sort.path",
                None,
                Some(id(83)),
                Condition::Predicate(query::Predicate::Registered {
                    id: "pred.missing".into(),
                    revision: 1,
                    operand: Vec::new(),
                })
            ),
            &CallerCtx::new(Channel::Gui, id(14), None),
        ),
        Err(SearchError::InvalidRequest)
    ));
    assert_eq!(
        runtime.state(&active.job_ref),
        Ok(du_core_contracts::job::JobState::Running)
    );
    gate.release().map_err(str::to_owned)?;
    test_gate::set(None).map_err(str::to_owned)?;
    assert_eq!(
        wait_terminal(&runtime, &active.job_ref)?.state,
        du_core_contracts::job::JobState::Done
    );
    test_gate::set(None).map_err(str::to_owned)?;

    // A valid replacement stops the old region job and only the new completion publishes.
    let gate = test_gate::Gate::new(test_gate::Stage::Worker);
    test_gate::set(Some((id(83), gate.clone()))).map_err(str::to_owned)?;
    let old_job = entry
        .execute_async(
            make_request(11, "sort.path", None, Some(id(83)), Condition::All(vec![])),
            &CallerCtx::new(Channel::Gui, id(12), None),
        )
        .map_err(|error| format!("old region async: {error:?}"))?;
    gate.entered().map_err(str::to_owned)?;
    test_gate::set(None).map_err(str::to_owned)?;
    let new_job = entry
        .execute_async(
            make_request(12, "sort.path", None, Some(id(83)), Condition::All(vec![])),
            &CallerCtx::new(Channel::Gui, id(13), None),
        )
        .map_err(|error| format!("replacement region async: {error:?}"))?;
    let new_terminal = wait_terminal(&runtime, &new_job.job_ref)?;
    assert_eq!(new_terminal.state, du_core_contracts::job::JobState::Done);
    assert!(new_terminal.result_version_id.is_some());
    gate.release().map_err(str::to_owned)?;
    let old_terminal = wait_terminal(&runtime, &old_job.job_ref)?;
    assert_eq!(
        old_terminal.state,
        du_core_contracts::job::JobState::Cancelled
    );
    assert_eq!(old_terminal.result_version_id, None);
    assert_eq!(
        finished_event(&events, &old_job.job_ref)?["body"]["queryRef"],
        serde_json::Value::Null
    );

    Ok(())
}
