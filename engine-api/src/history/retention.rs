//! GUI use case for reading the startup history retention policy.

use du_core_contracts::history_retention_ipc::{
    HistoryRetentionGetRequest, HistoryRetentionPolicy,
};

use crate::api::{CallerCtx, Channel, EngineBuilder, UseCase};
use crate::error::EngineError;

/// Registers `history.retention_get` for the GUI channel with the host-read policy.
pub fn register(builder: &mut EngineBuilder, policy: HistoryRetentionPolicy) {
    builder.register(GetRetention(policy));
}

struct GetRetention(HistoryRetentionPolicy);

impl UseCase for GetRetention {
    const NAME: &'static str = "history.retention_get";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = HistoryRetentionGetRequest;
    type Response = HistoryRetentionPolicy;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{CallerCtx, Channel, EngineBuilder};
    use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, ErrorCode, Id};
    use serde_json::json;

    fn policy() -> HistoryRetentionPolicy {
        HistoryRetentionPolicy {
            enabled: false,
            specified_days: None,
            suggested_days: 90,
            policy_fingerprint: "0123456789abcdef0123456789abcdef".into(),
            unavailable_reason: None,
        }
    }

    #[test]
    fn retention_get_returns_the_host_policy_only_to_gui() -> Result<(), String> {
        let mut builder = EngineBuilder::default();
        register(&mut builder, policy());
        let engine = builder.build().map_err(|error| error.to_string())?;
        let id = Id::from_v7_bytes([1; 16]);
        let call = |channel| {
            engine.call(
                CallEnvelope {
                    name: "history.retention_get".into(),
                    contract_version: ENVELOPE_VERSION,
                    correlation_id: id.clone(),
                    body: json!({}),
                },
                &CallerCtx::new(channel, id.clone(), None),
            )
        };

        let gui = call(Channel::Gui);
        assert_eq!(gui.error, None);
        assert_eq!(gui.body["suggestedDays"], 90);
        assert_eq!(
            gui.body["policyFingerprint"],
            "0123456789abcdef0123456789abcdef"
        );
        for channel in [Channel::Cli, Channel::Mcp] {
            assert_eq!(
                call(channel).error.map(|error| error.code),
                Some(ErrorCode::InvalidArgument)
            );
        }
        Ok(())
    }
}
