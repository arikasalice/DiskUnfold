use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, ErrorEnvelope, Id, ResultEnvelope};
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

pub(crate) fn dispatch(message: &str) -> Option<String> {
    let request: Request = serde_json::from_str(message).ok()?;
    let address: Address = serde_json::from_value(request.args.envelope.clone()).ok()?;
    let reason = if request.command != "du_call" {
        "command.channel_denied"
    } else if serde_json::from_value::<CallEnvelope>(request.args.envelope)
        .is_ok_and(|envelope| envelope.contract_version == ENVELOPE_VERSION)
    {
        "command.not_provided"
    } else {
        "command.envelope_invalid"
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
    use serde_json::json;

    #[test]
    fn only_the_call_channel_is_routed_and_unknown_commands_fail()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
        for (command, version, reason) in [
            ("du_call", 1, "command.not_provided"),
            ("du_call", 2, "command.envelope_invalid"),
            ("other", 1, "command.channel_denied"),
        ] {
            let request = json!({"command": command, "args": {"envelope": {
                "name": "app.product_info", "contractVersion": version, "correlationId": id, "body": {},
            }}});
            let reply: Value =
                serde_json::from_str(&dispatch(&request.to_string()).ok_or("missing response")?)?;
            assert_eq!(reply["correlationId"], id);
            assert_eq!(reply["error"]["reasonKey"], reason);
        }
        assert!(dispatch("{}").is_none());
        Ok(())
    }
}
