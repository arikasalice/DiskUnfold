use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, ErrorEnvelope, Id, ResultEnvelope};
use du_engine_api::api::{CallerCtx, Channel, Engine};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Request {
    command: String,
    args: Arguments,
}

#[derive(Deserialize)]
struct Arguments {
    envelope: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Address {
    name: String,
    correlation_id: Id,
}

pub(crate) fn dispatch(engine: &Engine, message: &str) -> Option<String> {
    let request: Request = serde_json::from_str(message).ok()?;
    let address: Address = serde_json::from_value(request.args.envelope.clone()).ok()?;
    let reason = if request.command != "du_call" {
        "command.channel_denied"
    } else {
        match serde_json::from_value::<CallEnvelope>(request.args.envelope) {
            Ok(mut envelope) => {
                if envelope.name == "job.get" {
                    envelope.name = "job.status".into();
                }
                let caller = CallerCtx::new(Channel::Gui, address.correlation_id, None);
                let mut response = engine.call(envelope, &caller);
                response.name = address.name;
                return serde_json::to_string(&response).ok();
            }
            Err(_) => "command.envelope_invalid",
        }
    };
    let error = ErrorEnvelope::from_reason(reason, address.correlation_id.clone(), Vec::new())?;
    serde_json::to_string(&ResultEnvelope {
        name: address.name,
        contract_version: ENVELOPE_VERSION,
        correlation_id: address.correlation_id,
        body: Value::Null,
        error: Some(error),
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_engine_api::{
        api::{EngineBuilder, UseCase},
        error::EngineError,
    };
    use serde_json::json;

    #[test]
    fn only_the_call_channel_is_routed_and_unknown_commands_fail()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = EngineBuilder::default().build()?;
        let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
        for (command, version, reason) in [
            ("du_call", 1, "command.not_provided"),
            ("du_call", 2, "command.envelope_invalid"),
            ("other", 1, "command.channel_denied"),
        ] {
            let request = json!({"command": command, "args": {"envelope": {
                "name": "app.product_info", "contractVersion": version, "correlationId": id, "body": {},
            }}});
            let reply: Value = serde_json::from_str(
                &dispatch(&engine, &request.to_string()).ok_or("missing response")?,
            )?;
            assert_eq!(reply["correlationId"], id);
            assert_eq!(reply["error"]["reasonKey"], reason);
        }
        assert!(dispatch(&engine, "{}").is_none());
        Ok(())
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TestRequest {
        count: u32,
    }

    struct TestUseCase;

    impl UseCase for TestUseCase {
        const NAME: &'static str = "job.status";
        const CHANNELS: &'static [Channel] = &[Channel::Gui];
        type Request = TestRequest;
        type Response = u32;

        fn check_scope(&self, _: &TestRequest, _: &CallerCtx) -> Result<(), EngineError> {
            Ok(())
        }

        fn execute(&self, request: TestRequest, _: &CallerCtx) -> Result<u32, EngineError> {
            Ok(request.count)
        }
    }

    #[test]
    fn routes_job_get_to_engine_status_and_preserves_the_ipc_name()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut builder = EngineBuilder::default();
        builder.register(TestUseCase);
        let engine = builder.build()?;
        let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
        let request = json!({"command": "du_call", "args": {"envelope": {
            "name": "job.get", "contractVersion": 1, "correlationId": id, "body": {"count": 3},
        }}});
        let reply: Value = serde_json::from_str(
            &dispatch(&engine, &request.to_string()).ok_or("missing response")?,
        )?;
        assert_eq!(reply["body"], 3);
        assert_eq!(reply["name"], "job.get");
        assert_eq!(reply["correlationId"], id);
        assert!(reply.get("error").is_none());
        Ok(())
    }
}
