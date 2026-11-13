use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, ResultEnvelope};
use serde_json::Value;

use crate::{
    contracts::{CallerCtx, Channel, OutboundFilter},
    error::EngineError,
    registry::Registry,
};

pub(crate) fn call(
    registry: &Registry,
    filter: Option<&dyn OutboundFilter>,
    envelope: CallEnvelope,
    caller: &CallerCtx,
) -> ResultEnvelope {
    let CallEnvelope {
        name,
        contract_version,
        correlation_id,
        body,
    } = envelope;
    let result = if contract_version == ENVELOPE_VERSION && correlation_id == caller.correlation_id
    {
        invoke(registry, filter, &name, body, caller)
    } else {
        Err(EngineError::reason("command.envelope_invalid"))
    };
    let (body, error) = match result {
        Ok(body) => (body, None),
        Err(error) => (
            Value::Null,
            Some(error.into_envelope(caller.correlation_id.clone())),
        ),
    };
    ResultEnvelope {
        name,
        contract_version: ENVELOPE_VERSION,
        correlation_id: caller.correlation_id.clone(),
        body,
        error,
    }
}

fn invoke(
    registry: &Registry,
    filter: Option<&dyn OutboundFilter>,
    name: &str,
    body: Value,
    caller: &CallerCtx,
) -> Result<Value, EngineError> {
    let not_provided = || EngineError::reason("command.not_provided");
    let use_case = registry.get(name).ok_or_else(not_provided)?;
    if !use_case.channels().contains(&caller.channel) {
        return Err(not_provided());
    }
    if caller.channel == Channel::Gui {
        // GUI command contracts use object request bodies. Check the container
        // before Serde, which also accepts positional arrays for Rust structs.
        if !body.is_object() {
            return Err(EngineError::reason("command.body_invalid"));
        }
        return use_case.invoke(body, caller);
    }

    let filter = filter.ok_or_else(not_provided)?;
    let response = use_case.invoke(body, caller)?;
    filter.filter(name, caller, response)
}
