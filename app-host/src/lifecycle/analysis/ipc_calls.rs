//! Engine IPC calls and result readers shared by the platform scan tests.
use super::*;
use du_core_contracts::{
    EventEnvelope, Id, ResultEnvelope, SizeAmount, SizeBasis, analysis_ipc::SpaceTotal,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

type Failure = Box<dyn std::error::Error>;

fn ipc(engine: &Engine, name: &str, body: Value) -> Result<ResultEnvelope, Failure> {
    let id = Id::from_v7_bytes([42; 16]);
    let message = json!({
        "command": "du_call",
        "args": {"envelope": {
            "name": name, "contractVersion": 1, "correlationId": id, "body": body
        }}
    });
    let response = crate::ipc::dispatch(engine, &message.to_string()).ok_or("no response")?;
    Ok(serde_json::from_str(&response)?)
}

/// Calls one GUI use case and decodes its body, failing on an error envelope.
pub(super) fn read<T: serde::de::DeserializeOwned>(
    engine: &Engine,
    name: &str,
    body: Value,
) -> Result<T, Failure> {
    let response = ipc(engine, name, body)?;
    if let Some(error) = response.error {
        return Err(format!("{name}: {error:?}").into());
    }
    Ok(serde_json::from_value(response.body)?)
}

/// Waits up to 20 seconds for the `job.finished` event of `job`.
pub(super) fn finished(events: &DesktopEvents, job: &Id) -> Result<EventEnvelope, Failure> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        match events.next_json() {
            Some(json) => {
                let event: EventEnvelope = serde_json::from_str(&json)?;
                if event.topic == "job.finished" && event.job_ref.as_ref() == Some(job) {
                    return Ok(event);
                }
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    Err("timed out waiting for the scan to finish".into())
}

/// Returns the logical bytes and whether the total is exact rather than a lower bound.
pub(super) fn logical(total: &SpaceTotal) -> Result<(u64, bool), Failure> {
    let size = total
        .sizes
        .iter()
        .find(|value| value.size.basis == SizeBasis::Logical)
        .ok_or("missing logical size")?;
    match &size.size.amount {
        SizeAmount::Exact { bytes } => Ok((bytes.as_str().parse()?, true)),
        SizeAmount::LowerBound { bytes } => Ok((bytes.as_str().parse()?, false)),
        other => Err(format!("unexpected logical size: {other:?}").into()),
    }
}
