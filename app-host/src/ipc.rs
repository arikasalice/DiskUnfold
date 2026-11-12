use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode, ErrorEnvelope, Id, ResultEnvelope,
    analysis_ipc::{AnalysisJobRequest, ScanStartRequest},
    startup_ipc::StartupAccessState,
};
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

/// Keeps query and job controls independent of potentially blocking requests.
/// The dispatcher still validates the entire envelope and channel.
#[cfg(windows)]
pub(crate) fn dispatch_lane(message: &str) -> du_platform_os::webview::DispatchLane {
    use du_platform_os::webview::DispatchLane;

    match gui_command_name(message).as_deref() {
        Some("target.list" | "scope.recent_open") => DispatchLane::Volume,
        Some("query.cancel" | "query.region.close" | "viz.cancel" | "viz.view.close") => {
            DispatchLane::QueryControl
        }
        Some("job.cancel" | "job.pause" | "job.resume") => DispatchLane::JobControl,
        _ => DispatchLane::Ordered,
    }
}

/// Reads the GUI command name for routing without decoding the request body.
#[cfg(windows)]
pub(crate) fn gui_command_name(message: &str) -> Option<std::borrow::Cow<'_, str>> {
    use std::borrow::Cow;

    #[derive(Deserialize)]
    struct Route<'a> {
        #[serde(borrow)]
        command: Cow<'a, str>,
        #[serde(borrow)]
        args: RouteArguments<'a>,
    }

    #[derive(Deserialize)]
    struct RouteArguments<'a> {
        #[serde(borrow)]
        envelope: RouteEnvelope<'a>,
    }

    #[derive(Deserialize)]
    struct RouteEnvelope<'a> {
        #[serde(borrow)]
        name: Cow<'a, str>,
    }

    let route = serde_json::from_str::<Route<'_>>(message).ok()?;
    (route.command == "du_call").then_some(route.args.envelope.name)
}

pub(crate) fn dispatch(engine: &Engine, message: &str) -> Option<String> {
    dispatch_with_access_state(engine, message, StartupAccessState::NotApplicable)
}

/// Dispatches a GUI request using the current startup access observation.
pub(crate) fn dispatch_with_startup_access(
    engine: &Engine,
    message: &str,
    access: &crate::startup_checks::StartupAccessSession,
) -> Option<String> {
    dispatch_with_access_state(engine, message, access.current())
}

fn dispatch_with_access_state(
    engine: &Engine,
    message: &str,
    access: StartupAccessState,
) -> Option<String> {
    let request: Request = serde_json::from_str(message).ok()?;
    let address = Address::deserialize(&request.args.envelope).ok()?;
    let reason = if request.command != "du_call" {
        "command.channel_denied"
    } else {
        match serde_json::from_value::<CallEnvelope>(request.args.envelope) {
            Ok(mut envelope) => {
                if envelope.name == "job.get" {
                    envelope.name = "job.status".into();
                }
                let caller = CallerCtx::new(Channel::Gui, address.correlation_id, None);
                if access == StartupAccessState::Denied {
                    if envelope.name == "scan.start"
                        && serde_json::from_value::<ScanStartRequest>(envelope.body.clone()).is_ok()
                    {
                        return serialize_analysis_denial(envelope);
                    }
                    if envelope.name == "job.resume"
                        && serde_json::from_value::<AnalysisJobRequest>(envelope.body.clone())
                            .is_ok()
                    {
                        let status_envelope = CallEnvelope {
                            name: "job.status".into(),
                            contract_version: envelope.contract_version,
                            correlation_id: envelope.correlation_id.clone(),
                            body: envelope.body.clone(),
                        };
                        let mut status = engine.call(status_envelope, &caller);
                        if status.error.is_some() {
                            status.name = address.name;
                            return serde_json::to_string(&status).ok();
                        }
                        let Some(kind) = status.body.get("kind").and_then(Value::as_str) else {
                            return serialize_analysis_denial(envelope);
                        };
                        if !resume_allowed_without_full_disk_access(kind) {
                            return serialize_analysis_denial(envelope);
                        }
                    }
                }
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

// These explicit lists keep the common job.resume command scoped to known runtime kinds.
// Any new kind is denied until the composition owner classifies it for this gate.
const ANALYSIS_JOB_KINDS: &[&str] = &["scan.metadata", "duplicate.check"];
const NON_ANALYSIS_RESUMABLE_JOB_KINDS: &[&str] = &["target.probe", "query.evaluate"];

fn resume_allowed_without_full_disk_access(kind: &str) -> bool {
    NON_ANALYSIS_RESUMABLE_JOB_KINDS.contains(&kind) && !ANALYSIS_JOB_KINDS.contains(&kind)
}

fn serialize_analysis_denial(envelope: CallEnvelope) -> Option<String> {
    serde_json::to_string(&ResultEnvelope {
        name: envelope.name,
        contract_version: ENVELOPE_VERSION,
        correlation_id: envelope.correlation_id.clone(),
        body: Value::Null,
        error: Some(ErrorEnvelope {
            code: ErrorCode::InvalidArgument,
            reason_key: "startup.full_disk_access_required".into(),
            message_key: "startup.full_disk_access_required".into(),
            correlation_id: envelope.correlation_id,
            item_refs: Vec::new(),
            retryable: false,
        }),
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{analysis_ipc::AnalysisJobRequest, startup::FullDiskAccessStatus};
    use du_engine_api::{
        api::{EngineBuilder, UseCase},
        error::EngineError,
    };
    use du_platform_os::startup::{
        FakeStartupCapabilities, PrivacySettingsOpenOutcome, StartupDialogOutcome,
        WebViewAvailability,
    };
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[cfg(windows)]
    #[test]
    fn selects_only_exact_valid_route_names() {
        use du_platform_os::webview::DispatchLane::{JobControl, Ordered, QueryControl, Volume};
        for (body, expected) in [
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"target.list"}}}"#,
                Volume,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"target\u002elist"}}}"#,
                Volume,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"query.cancel"}}}"#,
                QueryControl,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"query.region.close"}}}"#,
                QueryControl,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"viz.cancel"}}}"#,
                QueryControl,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"viz.view.close"}}}"#,
                QueryControl,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"viz.render"}}}"#,
                Ordered,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"browse.open"}}}"#,
                Ordered,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"job.cancel"}}}"#,
                JobControl,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"job.pause"}}}"#,
                JobControl,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"job.resume"}}}"#,
                JobControl,
            ),
            (
                r#"{"command":"du_call","args":{"envelope":{"name":"job.pause.extra"}}}"#,
                Ordered,
            ),
            (
                r#"{"command":"other","args":{"envelope":{"name":"job.cancel"}}}"#,
                Ordered,
            ),
            (
                r#"{"command":"other","args":{"envelope":{"name":"query.cancel"}}}"#,
                Ordered,
            ),
            (r#"{"name":"query.cancel"}"#, Ordered),
        ] {
            assert_eq!(dispatch_lane(body), expected);
        }
    }

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

    #[test]
    fn address_projection_preserves_envelope_validation_precedence()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = EngineBuilder::default().build()?;
        let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
        for (request, expected) in [
            (
                json!({"command":"other", "args":{"envelope":{"name":"x", "correlationId":id}}}),
                "command.channel_denied",
            ),
            (
                json!({"command":"du_call", "args":{"envelope":{"name":"x", "correlationId":id}}}),
                "command.envelope_invalid",
            ),
            (
                json!({"command":"other", "args":{"envelope":["x", id]}}),
                "command.channel_denied",
            ),
            (
                json!({"command":"du_call", "args":{"envelope":["x", id]}}),
                "command.envelope_invalid",
            ),
        ] {
            let reply: Value = serde_json::from_str(
                &dispatch(&engine, &request.to_string()).ok_or("missing response")?,
            )?;
            assert_eq!(reply["name"], "x");
            assert_eq!(reply["correlationId"], id);
            assert_eq!(reply["error"]["reasonKey"], expected);
        }

        for envelope in [
            json!({"correlationId":id}),
            json!({"name":"x"}),
            json!({"name":1, "correlationId":id}),
            json!({"name":"x", "correlationId":"not-an-id"}),
        ] {
            let request = json!({"command":"other", "args":{"envelope":envelope}});
            assert!(dispatch(&engine, &request.to_string()).is_none());
        }
        Ok(())
    }

    #[test]
    fn address_deserialization_ignores_envelope_extensions()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = EngineBuilder::default().build()?;
        let envelope = json!({
            "name":"job.get",
            "contractVersion":1,
            "correlationId":"0195f856-ab83-7a59-9343-10d29775c9a9",
            "body":{},
            "future":{"large":[1,2,3]},
        });
        let request = json!({"command":"du_call", "args":{"envelope":envelope}});
        let reply: Value = serde_json::from_str(
            &dispatch(&engine, &request.to_string()).ok_or("missing response")?,
        )?;
        assert_eq!(reply["name"], "job.get");
        assert_eq!(
            reply["correlationId"],
            "0195f856-ab83-7a59-9343-10d29775c9a9"
        );
        assert_eq!(reply["error"]["reasonKey"], "command.not_provided");
        Ok(())
    }

    struct TestUseCase(Arc<AtomicUsize>);

    impl UseCase for TestUseCase {
        const NAME: &'static str = "job.status";
        const CHANNELS: &'static [Channel] = &[Channel::Gui];
        type Request = AnalysisJobRequest;
        type Response = String;

        fn check_scope(&self, _: &AnalysisJobRequest, _: &CallerCtx) -> Result<(), EngineError> {
            Ok(())
        }

        fn execute(
            &self,
            request: AnalysisJobRequest,
            _: &CallerCtx,
        ) -> Result<String, EngineError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(request.job_ref.as_str().to_owned())
        }
    }

    struct ScanStartCounter(Arc<AtomicUsize>);

    impl UseCase for ScanStartCounter {
        const NAME: &'static str = "scan.start";
        const CHANNELS: &'static [Channel] = &[Channel::Gui];
        type Request = ScanStartRequest;
        type Response = Value;

        fn check_scope(&self, _: &ScanStartRequest, _: &CallerCtx) -> Result<(), EngineError> {
            Ok(())
        }

        fn execute(&self, _: ScanStartRequest, _: &CallerCtx) -> Result<Value, EngineError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"admitted": true}))
        }
    }

    struct ResumeCounter(Arc<AtomicUsize>);

    impl UseCase for ResumeCounter {
        const NAME: &'static str = "job.resume";
        const CHANNELS: &'static [Channel] = &[Channel::Gui];
        type Request = AnalysisJobRequest;
        type Response = Value;

        fn check_scope(&self, _: &AnalysisJobRequest, _: &CallerCtx) -> Result<(), EngineError> {
            Ok(())
        }

        fn execute(&self, _: AnalysisJobRequest, _: &CallerCtx) -> Result<Value, EngineError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"resumed": true}))
        }
    }

    struct StatusKind {
        kind: Option<&'static str>,
        fail: bool,
    }

    impl UseCase for StatusKind {
        const NAME: &'static str = "job.status";
        const CHANNELS: &'static [Channel] = &[Channel::Gui];
        type Request = AnalysisJobRequest;
        type Response = Value;

        fn check_scope(&self, _: &AnalysisJobRequest, _: &CallerCtx) -> Result<(), EngineError> {
            Ok(())
        }

        fn execute(&self, _: AnalysisJobRequest, _: &CallerCtx) -> Result<Value, EngineError> {
            if self.fail {
                return Err(EngineError::new("job.not_found", Vec::new()));
            }
            Ok(json!({"kind": self.kind}))
        }
    }

    fn denied_access() -> crate::startup_checks::StartupAccessSession {
        let capabilities = Arc::new(FakeStartupCapabilities::new(
            FullDiskAccessStatus::Denied,
            WebViewAvailability::Unknown,
            PrivacySettingsOpenOutcome::RequestAccepted,
            StartupDialogOutcome::Acknowledged,
        ));
        crate::startup_checks::StartupAccessSession::new(capabilities)
    }

    fn call_message(name: &str, body: Value) -> Value {
        json!({"command":"du_call", "args":{"envelope":{
            "name":name,
            "contractVersion":1,
            "correlationId":"0195f856-ab83-7a59-9343-10d29775c9a9",
            "body":body,
        }}})
    }

    #[test]
    fn denied_full_disk_access_blocks_valid_scan_start_before_engine_admission()
    -> Result<(), Box<dyn std::error::Error>> {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = EngineBuilder::default();
        builder.register(ScanStartCounter(calls.clone()));
        let engine = builder.build()?;
        let access = denied_access();
        let body = json!({
            "draft": {
                "includes": [], "exclusions": [], "boundary": "include_selected",
                "includedMounts": [], "source": "gui"
            },
            "reviewFingerprint": "reviewed"
        });
        let reply: Value = serde_json::from_str(
            &dispatch_with_startup_access(
                &engine,
                &call_message("scan.start", body).to_string(),
                &access,
            )
            .ok_or("missing denied scan response")?,
        )?;

        assert_eq!(
            reply["error"]["reasonKey"],
            "startup.full_disk_access_required"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        Ok(())
    }

    #[test]
    fn denied_access_allows_only_confirmed_non_analysis_resume_kinds()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
        for (kind, expected_reason, expected_resumes) in [
            (
                Some("scan.metadata"),
                "startup.full_disk_access_required",
                0,
            ),
            (Some("target.probe"), "", 1),
            (Some("query.evaluate"), "", 1),
            (
                Some("duplicate.check"),
                "startup.full_disk_access_required",
                0,
            ),
            (
                Some("future.analysis"),
                "startup.full_disk_access_required",
                0,
            ),
            (None, "startup.full_disk_access_required", 0),
        ] {
            let resumes = Arc::new(AtomicUsize::new(0));
            let mut builder = EngineBuilder::default();
            builder.register(StatusKind { kind, fail: false });
            builder.register(ResumeCounter(resumes.clone()));
            let engine = builder.build()?;
            let reply: Value = serde_json::from_str(
                &dispatch_with_startup_access(
                    &engine,
                    &call_message("job.resume", json!({"jobRef":id})).to_string(),
                    &denied_access(),
                )
                .ok_or("missing resume response")?,
            )?;

            if expected_reason.is_empty() {
                assert!(reply.get("error").is_none());
            } else {
                assert_eq!(reply["error"]["reasonKey"], expected_reason);
            }
            assert_eq!(resumes.load(Ordering::SeqCst), expected_resumes);
        }
        Ok(())
    }

    #[test]
    fn denied_access_fails_closed_when_resume_kind_cannot_be_read()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
        let resumes = Arc::new(AtomicUsize::new(0));
        let mut builder = EngineBuilder::default();
        builder.register(StatusKind {
            kind: None,
            fail: true,
        });
        builder.register(ResumeCounter(resumes.clone()));
        let engine = builder.build()?;
        let reply: Value = serde_json::from_str(
            &dispatch_with_startup_access(
                &engine,
                &call_message("job.resume", json!({"jobRef":id})).to_string(),
                &denied_access(),
            )
            .ok_or("missing status failure response")?,
        )?;

        assert_eq!(reply["error"]["reasonKey"], "job.not_found");
        assert_eq!(reply["name"], "job.resume");
        assert_eq!(resumes.load(Ordering::SeqCst), 0);
        Ok(())
    }

    #[test]
    fn routes_job_get_to_engine_status_and_preserves_the_ipc_name()
    -> Result<(), Box<dyn std::error::Error>> {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut builder = EngineBuilder::default();
        builder.register(TestUseCase(dispatches.clone()));
        let engine = builder.build()?;
        let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
        let request = json!({"command": "du_call", "args": {"envelope": {
            "name": "job.get", "contractVersion": 1, "correlationId": id, "body": {"jobRef": id},
        }}});
        let reply: Value = serde_json::from_str(
            &dispatch(&engine, &request.to_string()).ok_or("missing response")?,
        )?;
        assert_eq!(reply["body"], id);
        assert_eq!(reply["name"], "job.get");
        assert_eq!(reply["correlationId"], id);
        assert!(reply.get("error").is_none());
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);

        for body in [
            json!([id]),
            Value::Null,
            json!(id),
            json!(7),
            json!({"jobRef": "invalid"}),
        ] {
            let malformed = json!({"command":"du_call", "args":{"envelope":{
                "name":"job.get", "contractVersion":1, "correlationId":id, "body":body,
            }}});
            let reply: Value = serde_json::from_str(
                &dispatch(&engine, &malformed.to_string())
                    .ok_or("missing malformed-body response")?,
            )?;
            assert_eq!(reply["name"], "job.get");
            assert_eq!(reply["correlationId"], id);
            assert_eq!(reply["error"]["reasonKey"], "command.body_invalid");
            assert_eq!(dispatches.load(Ordering::SeqCst), 1);
        }

        for (command, version, expected) in [
            ("other", 1, "command.channel_denied"),
            ("du_call", 2, "command.envelope_invalid"),
        ] {
            let request = json!({"command":command, "args":{"envelope":{
                "name":"job.get", "contractVersion":version, "correlationId":id, "body":[id],
            }}});
            let reply: Value = serde_json::from_str(
                &dispatch(&engine, &request.to_string()).ok_or("missing precedence response")?,
            )?;
            assert_eq!(reply["name"], "job.get");
            assert_eq!(reply["correlationId"], id);
            assert_eq!(reply["error"]["reasonKey"], expected);
            assert_eq!(dispatches.load(Ordering::SeqCst), 1);
        }
        Ok(())
    }
}
