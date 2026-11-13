use super::*;
use crate::{
    api::{CallerCtx, Channel},
    search::{
        test_gate,
        test_helpers::{finished_event, job_status, wait_terminal},
    },
};
use serde_json::json;
use std::time::Duration;

#[test]
fn publisher_wins_query_cancel_without_region_stop_or_result_loss() -> Result<(), String> {
    let _serial = test_gate::serial().map_err(str::to_owned)?;
    let _cleanup = ClearTestGates;
    let harness = make_harness()?;
    let browse_ref = browse_query(&harness)?;

    let publish = test_gate::Gate::new(test_gate::Stage::Publish);
    test_gate::set(Some((harness.version.clone(), publish.clone()))).map_err(str::to_owned)?;
    let admitted = harness
        .entry
        .execute_async(
            search_request(harness.search_region.clone(), harness.version.clone()),
            &CallerCtx::new(Channel::Gui, id(216), None),
        )
        .map_err(|error| format!("search admission: {error:?}"))?;
    publish.entered().map_err(|error| {
        format!(
            "search publish barrier: {error}; state={:?}",
            harness
                .runtime
                .snapshot(&admitted.job_ref)
                .map(|snapshot| snapshot.state)
        )
    })?;
    let cancel_gate = test_gate::Gate::new(test_gate::Stage::CancelPreflight);
    test_gate::set_cancel_gate(Some((admitted.job_ref.clone(), cancel_gate.clone())))
        .map_err(str::to_owned)?;
    let cancel_engine = harness.engine.clone();
    let cancel_region = harness.search_region.clone();
    let (response_tx, response_rx) = std::sync::mpsc::channel();
    let cancel_worker = std::thread::spawn(move || {
        response_tx
            .send(request_cancel(
                &cancel_engine,
                &cancel_region,
                admitted.request_seq,
            ))
            .map_err(|error| error.to_string())
    });
    if let Err(error) = cancel_gate.entered() {
        return Err(format!(
            "cancel preflight barrier: {error}; response={:?}",
            response_rx.try_recv()
        ));
    }
    assert!(
        !harness
            .entry
            .regions
            .registry
            .job_is_stopped(
                &harness.entry.regions.owner,
                &harness.search_region,
                admitted.request_seq,
                &admitted.job_ref,
            )
            .map_err(|error| format!("region state: {error:?}"))?
    );
    cancel_gate.release().map_err(str::to_owned)?;
    assert!(
        matches!(
            response_rx.recv_timeout(Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "cancel returned while publisher owned the gate"
    );
    assert!(
        !harness
            .entry
            .regions
            .registry
            .job_is_stopped(
                &harness.entry.regions.owner,
                &harness.search_region,
                admitted.request_seq,
                &admitted.job_ref,
            )
            .map_err(|error| format!("region state: {error:?}"))?
    );

    publish.release().map_err(str::to_owned)?;
    let terminal = wait_terminal(&harness.runtime, &admitted.job_ref)?;
    assert_eq!(terminal.state, du_core_contracts::job::JobState::Done);
    let query_ref = terminal
        .result_version_id
        .clone()
        .ok_or("missing query result")?;
    let cancelled = response_rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|error| error.to_string())?;
    cancel_worker
        .join()
        .map_err(|_| "query.cancel thread panicked".to_owned())?
        .map_err(|error| error.to_string())?;
    assert!(cancelled.error.is_none(), "{:?}", cancelled.error);
    assert_eq!(cancelled.body["outcome"], "already_terminal");
    test_gate::set_cancel_gate(None).map_err(str::to_owned)?;
    let status = job_status(&harness.engine, &admitted.job_ref)?;
    let finished = finished_event(&harness.events, &admitted.job_ref)?;
    assert_eq!(status["state"], "done");
    assert_eq!(finished["body"]["state"], status["state"]);
    assert_eq!(status["queryRef"], json!(query_ref));
    assert_eq!(finished["body"]["queryRef"], query_ref.as_str());
    let browse_page = call(
        &harness.engine,
        "query.page",
        json!({"queryRef":browse_ref,"resultVersion":harness.version,"start":0,"count":10}),
        217,
    );
    assert!(
        browse_page.error.is_none(),
        "Browse result lost: {:?}",
        browse_page.error
    );
    assert_eq!(browse_page.body["rowKind"], "folder");
    let repeated = request_cancel(
        &harness.engine,
        &harness.search_region,
        admitted.request_seq,
    );
    assert!(repeated.error.is_none(), "{:?}", repeated.error);
    assert_eq!(repeated.body["outcome"], "already_terminal");
    Ok(())
}
