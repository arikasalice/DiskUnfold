use super::*;
use crate::{
    api::{CallerCtx, Channel},
    search::{
        test_gate,
        test_helpers::{finished_event, job_status, wait_terminal},
    },
};
use serde_json::json;

#[test]
fn request_cancel_racing_job_binding_cancels_through_f24_without_stopping_early()
-> Result<(), String> {
    let _serial = test_gate::serial().map_err(str::to_owned)?;
    let _cleanup = ClearTestGates;
    let harness = make_harness()?;
    let browse_ref = browse_query(&harness)?;

    // Hold job admission after F24 creates the queued job but before the region binds it.
    let admission = test_gate::Gate::new(test_gate::Stage::Admission);
    test_gate::set(Some((harness.version.clone(), admission.clone()))).map_err(str::to_owned)?;
    let entry = harness.entry.clone();
    let region = harness.search_region.clone();
    let version = harness.version.clone();
    let admission_worker = std::thread::spawn(move || {
        entry.execute_async(
            search_request(region, version),
            &CallerCtx::new(Channel::Gui, id(214), None),
        )
    });
    admission
        .entered()
        .map_err(|error| format!("search admission barrier: {error}"))?;

    // Pause query.cancel before its atomic region decision while job admission is blocked.
    // Bind the admitted job before allowing that decision to take the region lock.
    let cancel_gate = test_gate::Gate::new(test_gate::Stage::RequestCancel);
    test_gate::set_cancel_gate(Some((harness.search_region.clone(), cancel_gate.clone())))
        .map_err(str::to_owned)?;
    let cancel_engine = harness.engine.clone();
    let cancel_region = harness.search_region.clone();
    let cancel_worker =
        std::thread::spawn(move || request_cancel(&cancel_engine, &cancel_region, 1));
    cancel_gate
        .entered()
        .map_err(|error| format!("request cancel barrier: {error}"))?;

    let candidate = test_gate::Gate::new(test_gate::Stage::Candidate);
    test_gate::set(Some((harness.version.clone(), candidate.clone()))).map_err(str::to_owned)?;
    admission.release().map_err(str::to_owned)?;
    let admitted = admission_worker
        .join()
        .map_err(|_| "search admission thread panicked".to_owned())?
        .map_err(|error| format!("search admission: {error:?}"))?;
    candidate
        .entered()
        .map_err(|error| format!("search candidate barrier: {error}"))?;
    let foreign = make_harness()?;
    let wrong_owner = request_cancel(
        &harness.engine,
        &foreign.search_region,
        admitted.request_seq,
    );
    assert_eq!(
        wrong_owner
            .error
            .map(|error| error.reason_key.as_str().to_owned()),
        Some("ref.unavailable".into())
    );
    assert_eq!(
        harness
            .runtime
            .snapshot(&admitted.job_ref)
            .map_err(|error| format!("wrong-owner state: {error:?}"))?
            .state,
        du_core_contracts::job::JobState::Running
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

    cancel_gate.release().map_err(str::to_owned)?;
    let cancelled = cancel_worker
        .join()
        .map_err(|_| "query.cancel thread panicked".to_owned())?;
    assert!(cancelled.error.is_none(), "{:?}", cancelled.error);
    assert_eq!(cancelled.body["outcome"], "accepted");
    test_gate::set_cancel_gate(None).map_err(str::to_owned)?;
    assert!(
        harness
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

    let repeated = request_cancel(
        &harness.engine,
        &harness.search_region,
        admitted.request_seq,
    );
    assert!(repeated.error.is_none(), "{:?}", repeated.error);
    assert_eq!(repeated.body["outcome"], "no_effect");
    candidate.release().map_err(str::to_owned)?;
    let terminal = wait_terminal(&harness.runtime, &admitted.job_ref)?;
    assert_eq!(terminal.state, du_core_contracts::job::JobState::Cancelled);
    assert_eq!(terminal.result_version_id, None);
    let status = job_status(&harness.engine, &admitted.job_ref)?;
    let finished = finished_event(&harness.events, &admitted.job_ref)?;
    assert_eq!(status["state"], "cancelled");
    assert_eq!(finished["body"]["state"], status["state"]);
    assert!(status["queryRef"].is_null());
    assert!(finished["body"]["queryRef"].is_null());
    let browse_page = call(
        &harness.engine,
        "query.page",
        json!({"queryRef":browse_ref,"resultVersion":harness.version,"start":0,"count":10}),
        215,
    );
    assert!(
        browse_page.error.is_none(),
        "Browse result lost: {:?}",
        browse_page.error
    );
    assert_eq!(browse_page.body["rowKind"], "folder");
    Ok(())
}
