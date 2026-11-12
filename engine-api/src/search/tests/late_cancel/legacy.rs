use super::*;
use crate::{
    api::{CallerCtx, Channel, EngineBuilder},
    browse::folder::{FolderBrowse, job::FolderJobs},
    desktop::DesktopEvents,
    query_region::{CancelAction, commands::Regions},
    search::{SearchEntry, SearchRequest, SearchWorkers, test_gate, test_helpers::wait_terminal},
    workers::Workers,
};
use du_analysis_index::{
    IngestSink, Observation,
    query::{self, Condition, SortSpec},
};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, query_region_ipc::QueryRegionKind, result::ResultCompleteness,
};
use du_job_runtime::JobRuntime;
use serde_json::json;
use std::sync::{Arc, atomic::AtomicUsize};
use std::time::Duration;

#[test]
fn legacy_region_stop_before_f24_gate_reproduces_failed_search_terminal() -> Result<(), String> {
    let _serial = test_gate::serial().map_err(str::to_owned)?;
    let store = IngestSink::default();
    let mut scan = store.open(id(1)).map_err(|error| format!("{error:?}"))?;
    scan.add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    scan.add_file(Observation {
        path: vec![b"item".to_vec()],
        size: None,
    })
    .map_err(|error| format!("{error:?}"))?;
    scan.seal(id(2), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;

    let sets = Arc::new(du_analysis_index::query::page::ResultSets::new(store));
    let ids = Arc::new(super::super::Identity {
        next: AtomicUsize::new(0),
        cancel_on: AtomicUsize::new(usize::MAX),
        replace_on: AtomicUsize::new(usize::MAX),
        caller: std::sync::Mutex::new(None),
        store: std::sync::Mutex::new(None),
    });
    let events = Arc::new(DesktopEvents::default());
    let runtime = Arc::new(
        JobRuntime::new(
            ids.clone(),
            Arc::new(TestClock),
            Arc::new(TestHistory),
            events,
        )
        .map_err(|error| format!("{error:?}"))?,
    );
    let mut builder = EngineBuilder::default();
    let regions = Regions::register(&mut builder, ids.clone(), &TestEntropy, runtime.clone())
        .map_err(|error| format!("{error:?}"))?;
    crate::jobs::register(&mut builder, runtime.clone());
    let region_ref = regions
        .registry
        .open(&regions.owner, QueryRegionKind::Search)
        .map_err(|error| format!("{error:?}"))?;
    let browse = Arc::new(FolderBrowse::with_result_sets(sets.clone(), ids.clone()));
    let jobs = Arc::new(FolderJobs::new(browse, runtime.clone()).map_err(|e| format!("{e:?}"))?);
    let workers = Arc::new(Workers::start(runtime.clone()).map_err(|e| format!("{e:?}"))?);
    let entry = SearchEntry::new(
        sets,
        regions.clone(),
        ids,
        SearchWorkers::new(jobs, workers),
    );
    let engine = Arc::new(builder.build().map_err(|error| error.to_string())?);

    let gate = test_gate::Gate::new(test_gate::Stage::Publish);
    test_gate::set(Some((id(2), gate.clone()))).map_err(str::to_owned)?;
    let job = entry
        .execute_async(
            SearchRequest {
                region_ref: region_ref.clone(),
                request_seq: 1,
                result_version: Some(id(2)),
                parent_search_ref: None,
                condition: Condition::All(Vec::new()),
                sorts: vec![SortSpec {
                    id: "sort.path".into(),
                    revision: 1,
                    basis: query::Basis::Logical,
                    descending: false,
                }],
                count: None,
            },
            &CallerCtx::new(Channel::Gui, id(3), None),
        )
        .map_err(|error| format!("search admission: {error:?}"))?;
    // This gate is inside F24's publication gate and before Permit::publish.
    gate.entered().map_err(str::to_owned)?;

    // Reproduce the region side of query.cancel's current two-step order. The actual
    // command then requests F24 cancellation while the worker still owns its gate.
    assert!(matches!(
        regions.registry.cancel_job(&regions.owner, &region_ref, &job.job_ref),
        Ok(CancelAction::Job(ref job_ref)) if job_ref == &job.job_ref
    ));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let cancel_engine = engine.clone();
    let cancel_region = region_ref.clone();
    let cancel_job = job.job_ref.clone();
    let cancel_call = std::thread::spawn(move || {
        let correlation_id = id(4);
        started_tx.send(()).map_err(|error| error.to_string())?;
        let response = cancel_engine.call(
            CallEnvelope {
                name: "query.cancel".into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: correlation_id.clone(),
                body: json!({
                    "regionRef": cancel_region,
                    "target": {"kind": "job", "jobRef": cancel_job},
                }),
            },
            &CallerCtx::new(Channel::Gui, correlation_id, None),
        );
        Ok::<_, String>(response)
    });
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|error| error.to_string())?;
    gate.release().map_err(str::to_owned)?;

    let response = cancel_call
        .join()
        .map_err(|_| "query.cancel thread panicked".to_owned())??;
    let terminal = wait_terminal(&runtime, &job.job_ref)?;
    test_gate::set(None).map_err(str::to_owned)?;

    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(response.body["outcome"], "already_terminal");
    assert_eq!(terminal.state, du_core_contracts::job::JobState::Failed);
    assert_eq!(terminal.result_version_id, None);
    assert_eq!(
        terminal.failure_reason_key.as_deref(),
        Some("support.internal_error")
    );
    Ok(())
}
