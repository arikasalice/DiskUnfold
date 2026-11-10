use std::sync::atomic::{AtomicUsize, Ordering};

use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, Id, ResultEnvelope};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{CallerCtx, Channel, Engine, EngineBuilder, OutboundFilter, UseCase},
    error::EngineError,
};

mod analysis;
mod scan_failures;
mod scan_flow;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    in_scope: bool,
}

struct Query<const GUI_ONLY: bool>;

impl<const GUI_ONLY: bool> UseCase for Query<GUI_ONLY> {
    const NAME: &'static str = "test.query";
    const CHANNELS: &'static [Channel] = if GUI_ONLY {
        &[Channel::Gui]
    } else {
        &[Channel::Gui, Channel::Cli, Channel::Mcp]
    };
    type Request = Request;
    type Response = Value;

    fn check_scope(&self, request: &Request, _: &CallerCtx) -> Result<(), EngineError> {
        if request.in_scope {
            Ok(())
        } else {
            Err(EngineError::reason("path.outside_scope"))
        }
    }

    fn execute(&self, _: Request, _: &CallerCtx) -> Result<Value, EngineError> {
        Ok(json!({"count": 3, "path": "local"}))
    }
}

struct Redact<'a>(&'a AtomicUsize);

impl OutboundFilter for Redact<'static> {
    fn filter(&self, _: &str, _: &CallerCtx, response: Value) -> Result<Value, EngineError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(json!({"count": response["count"]}))
    }
}

fn call(
    engine: &Engine,
    name: &str,
    channel: Channel,
    version: u32,
    body: Value,
) -> Result<ResultEnvelope, String> {
    let id = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let caller = CallerCtx::new(channel, id.clone(), None);
    let response = engine.call(
        CallEnvelope {
            name: name.into(),
            contract_version: version,
            correlation_id: id.clone(),
            body,
        },
        &caller,
    );
    assert_eq!(response.correlation_id, id);
    assert_eq!(response.contract_version, ENVELOPE_VERSION);
    Ok(response)
}

#[test]
fn duplicate_registration_fails_assembly() {
    let mut builder = EngineBuilder::default();
    builder.register(Query::<true>);
    builder.register(Query::<false>);
    assert_eq!(
        builder.build().err().map(|e| e.duplicate_name),
        Some("test.query")
    );
}

#[test]
fn rejects_in_pipeline_order_and_hides_unavailable_use_cases() -> Result<(), String> {
    let mut builder = EngineBuilder::default();
    builder.register(Query::<true>);
    let engine = builder.build().map_err(|e| format!("{e:?}"))?;
    for (name, channel, version, body, reason) in [
        (
            "missing",
            Channel::Gui,
            2,
            json!(null),
            "command.envelope_invalid",
        ),
        (
            "missing",
            Channel::Gui,
            1,
            json!(null),
            "command.not_provided",
        ),
        (
            "test.query",
            Channel::Mcp,
            1,
            json!(null),
            "command.not_provided",
        ),
        (
            "test.query",
            Channel::Gui,
            1,
            json!({"in_scope": false, "other": 1}),
            "command.body_invalid",
        ),
        (
            "test.query",
            Channel::Gui,
            1,
            json!({"in_scope": false}),
            "path.outside_scope",
        ),
    ] {
        let response = call(&engine, name, channel, version, body)?;
        assert!(response.body.is_null());
        let error = response.error.ok_or("expected an error envelope")?;
        assert_eq!(error.reason_key, reason);
        assert_eq!(error.correlation_id, response.correlation_id);
    }
    Ok(())
}

#[test]
fn cli_and_mcp_fail_closed_without_filter_and_filter_after_execution() -> Result<(), String> {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let mut unfiltered = EngineBuilder::default();
    unfiltered.register(Query::<false>);
    let mut filtered = EngineBuilder::default();
    filtered.register(Query::<false>);
    filtered.with_outbound_filter(Redact(&CALLS));
    let unfiltered = unfiltered.build().map_err(|e| format!("{e:?}"))?;
    let filtered = filtered.build().map_err(|e| format!("{e:?}"))?;
    for channel in [Channel::Cli, Channel::Mcp] {
        let body = json!({"in_scope": true});
        let response = call(
            &unfiltered,
            "test.query",
            channel,
            ENVELOPE_VERSION,
            body.clone(),
        )?;
        assert_eq!(
            response
                .error
                .ok_or("expected a missing command")?
                .reason_key,
            "command.not_provided"
        );
        let response = call(&filtered, "test.query", channel, ENVELOPE_VERSION, body)?;
        assert_eq!(response.error, None);
        assert_eq!(response.body, json!({"count": 3}));
    }
    let gui = call(
        &filtered,
        "test.query",
        Channel::Gui,
        ENVELOPE_VERSION,
        json!({"in_scope": true}),
    )?;
    assert_eq!(gui.error, None);
    assert_eq!(gui.body, json!({"count": 3, "path": "local"}));
    assert_eq!(CALLS.load(Ordering::Relaxed), 2);
    Ok(())
}
