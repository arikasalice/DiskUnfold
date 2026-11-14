//! The use cases through the engine pipeline: channels, bodies and error codes.

use std::sync::Arc;

use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode, ResultEnvelope,
    sample_ipc::{SampleSession, SampleSessionState},
};
use serde_json::{Value, json};

use super::{Rig, job_id};
use crate::{
    api::{CallerCtx, Channel, Engine, EngineBuilder},
    sample::register_with,
};

fn engine(rig: &Rig) -> Result<Engine, String> {
    let mut builder = EngineBuilder::default();
    register_with(
        &mut builder,
        rig.jobs.clone(),
        rig.loader.clone(),
        rig.sink.clone(),
    );
    builder.build().map_err(|error| format!("{error:?}"))
}

fn call(engine: &Engine, name: &str, channel: Channel, body: Value) -> ResultEnvelope {
    let id = job_id(7);
    engine.call(
        CallEnvelope {
            name: name.into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: id.clone(),
            body,
        },
        &CallerCtx::new(channel, id, None),
    )
}

#[test]
fn only_the_gui_channel_reaches_the_sample_commands() -> Result<(), String> {
    let rig = Rig::new();
    let engine = engine(&rig)?;
    let reference = json!({"sessionRef": job_id(1).as_str()});
    for name in [
        "sample.open",
        "sample.status",
        "sample.reset",
        "sample.close",
    ] {
        let body = if name == "sample.open" {
            json!({})
        } else {
            reference.clone()
        };
        for channel in [Channel::Cli, Channel::Mcp] {
            let response = call(&engine, name, channel, body.clone());
            let error = response.error.ok_or("expected a rejection")?;
            assert_eq!(error.reason_key, "command.not_provided", "{name}");
        }
    }
    assert_eq!((rig.submissions(), rig.events().len()), (0, 0));
    Ok(())
}

#[test]
fn open_status_reset_and_close_round_trip_over_the_wire() -> Result<(), String> {
    let rig = Rig::new();
    let engine = engine(&rig)?;
    let opened = call(&engine, "sample.open", Channel::Gui, json!({}));
    assert!(opened.error.is_none());
    let view: SampleSession = serde_json::from_value(opened.body).map_err(|e| e.to_string())?;
    assert_eq!(view.state, SampleSessionState::Preparing);
    let reference = json!({"sessionRef": view.session_ref.as_str()});
    let status = call(&engine, "sample.status", Channel::Gui, reference.clone());
    assert_eq!(status.body["state"], "preparing");
    assert_eq!(status.body["sequence"], 1);
    let reset = call(&engine, "sample.reset", Channel::Gui, reference.clone());
    assert_eq!(reset.body["jobRef"], status.body["jobRef"]);
    let closed = call(&engine, "sample.close", Channel::Gui, reference.clone());
    assert_eq!(closed.body["state"], "closed");
    assert_eq!(closed.body["releasedVersions"], 3);
    let gone = call(&engine, "sample.status", Channel::Gui, reference);
    let error = gone.error.ok_or("expected not found")?;
    assert_eq!(error.reason_key, "sample.session_closed");
    assert_eq!(error.code, ErrorCode::NotFound);
    Ok(())
}

#[test]
fn queue_pressure_is_busy_and_bodies_are_validated() -> Result<(), String> {
    let rig = Rig::new();
    let engine = engine(&rig)?;
    *rig.jobs.fail.lock().map_err(|e| e.to_string())? =
        Some(du_job_runtime::RuntimeError::QueueFull);
    let busy = call(&engine, "sample.open", Channel::Gui, json!({}));
    let error = busy.error.ok_or("expected busy")?;
    assert_eq!(
        (error.reason_key.as_str(), error.code),
        ("job.queue_full", ErrorCode::Busy)
    );
    for (name, body) in [
        ("sample.open", json!({"extra": 1})),
        ("sample.status", json!({})),
        (
            "sample.close",
            json!({"sessionRef": job_id(1).as_str(), "x": 1}),
        ),
    ] {
        let response = call(&engine, name, Channel::Gui, body);
        let error = response.error.ok_or("expected invalid body")?;
        assert_eq!(error.reason_key, "command.body_invalid", "{name}");
    }
    let _ = Arc::strong_count(&rig.loader);
    Ok(())
}
