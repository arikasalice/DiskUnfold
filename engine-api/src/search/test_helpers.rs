//! Small bounded helpers shared by the F04 async search tests.
use crate::{
    api::{CallerCtx, Channel, Engine},
    desktop::DesktopEvents,
};
use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, Id};
use du_job_runtime::{JobRuntime, JobSnapshot};
use serde_json::{Value, json};

pub(super) fn wait_terminal(runtime: &JobRuntime, job: &Id) -> Result<JobSnapshot, String> {
    for _ in 0..10_000 {
        let snapshot = runtime
            .snapshot(job)
            .map_err(|error| format!("{error:?}"))?;
        if snapshot.state.is_terminal() {
            return Ok(snapshot);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Err("job did not reach a terminal state".into())
}

pub(super) fn finished_event(events: &DesktopEvents, job: &Id) -> Result<Value, String> {
    for _ in 0..10_000 {
        if let Some(json) = events.next_json() {
            let event: Value = serde_json::from_str(&json).map_err(|error| error.to_string())?;
            if event["topic"] == "job.finished" && event["jobRef"] == job.as_str() {
                return Ok(event);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Err("job.finished event was not delivered".into())
}

pub(super) fn job_status(engine: &Engine, job: &Id) -> Result<Value, String> {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = 90;
    let correlation_id = Id::from_v7_bytes(bytes);
    let caller = CallerCtx::new(Channel::Gui, correlation_id.clone(), None);
    let response = engine.call(
        CallEnvelope {
            name: "job.status".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id,
            body: json!({ "jobRef": job }),
        },
        &caller,
    );
    if let Some(error) = response.error {
        return Err(format!("job.status: {}", error.reason_key));
    }
    Ok(response.body)
}
