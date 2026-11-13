use std::{
    ops::Deref,
    sync::{Arc, Mutex},
};

use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode, ErrorEnvelope, Id, ResultEnvelope,
    analysis_ipc::{AnalysisJobRequest, ScanStartRequest},
    shutdown_ipc::{AppQuitRequest, AppQuitResponse},
    startup_ipc::StartupAccessState,
};
use du_engine_api::api::{CallerCtx, Channel, Engine};
use serde::Deserialize;
use serde_json::Value;

type EngineSlot = Mutex<Option<Arc<Engine>>>;

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

/// Commands that stay available while the app is closing (F15 DLD §15): the
/// startup status, job queries and job cancellation. `app.quit` is handled
/// before this list is consulted.
const CLOSING_ALLOWED: &[&str] = &["app.startup_status", "job.list", "job.status", "job.cancel"];

/// Reason for commands refused while closing. The error is INVALID_ARGUMENT and
/// not retryable: the app is leaving and the same command can never succeed.
const CLOSING_REASON: &str = "startup.shutting_down";

/// Where a dispatch finds its engine. The slot form is read after the `app.quit`
/// and closing checks, and it answers with a typed error when the shutdown
/// has already released the engine.
pub(crate) enum EngineSource<'a> {
    Direct(&'a Engine),
    Slot(&'a EngineSlot),
}

impl<'a> From<&'a Engine> for EngineSource<'a> {
    fn from(engine: &'a Engine) -> Self {
        Self::Direct(engine)
    }
}

impl<'a> From<&'a EngineSlot> for EngineSource<'a> {
    fn from(slot: &'a EngineSlot) -> Self {
        Self::Slot(slot)
    }
}

enum Lease<'a> {
    Borrowed(&'a Engine),
    Owned(Arc<Engine>),
}

impl Deref for Lease<'_> {
    type Target = Engine;

    fn deref(&self) -> &Engine {
        match self {
            Self::Borrowed(engine) => engine,
            Self::Owned(engine) => engine,
        }
    }
}

enum Unavailable {
    Released,
    Poisoned,
}

impl EngineSource<'_> {
    fn lease(&self) -> Result<Lease<'_>, Unavailable> {
        match self {
            Self::Direct(engine) => Ok(Lease::Borrowed(engine)),
            Self::Slot(slot) => slot
                .lock()
                .map_err(|_| Unavailable::Poisoned)?
                .as_ref()
                .map(|engine| Lease::Owned(engine.clone()))
                .ok_or(Unavailable::Released),
        }
    }
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
        // Quit joins the job controls so a request blocked behind ordered work
        // cannot hold up leaving the app.
        Some("job.cancel" | "job.pause" | "job.resume" | "app.quit") => DispatchLane::JobControl,
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

#[cfg(test)]
pub(crate) fn dispatch(engine: &Engine, message: &str) -> Option<String> {
    dispatch_inner(engine, message, None, None)
}

/// Dispatches a GUI request using the current startup access observation.
#[cfg(test)]
pub(crate) fn dispatch_with_startup_access(
    engine: &Engine,
    message: &str,
    access: &crate::startup_checks::StartupAccessSession,
) -> Option<String> {
    dispatch_inner(engine, message, Some(access), None)
}

/// Dispatches a GUI request with startup access checks and the host shutdown controller.
///
/// The engine may be borrowed directly or read from the host's owner slot. Every
/// request receives a typed reply: after the shutdown released the engine, a
/// command receives the closing rejection instead of no response.
pub(crate) fn dispatch_with_shutdown<'a>(
    engine: impl Into<EngineSource<'a>>,
    message: &str,
    access: &crate::startup_checks::StartupAccessSession,
    shutdown: &dyn crate::shutdown::host::QuitHandler,
) -> Option<String> {
    dispatch_inner(engine, message, Some(access), Some(shutdown))
}

fn dispatch_inner<'a>(
    engine: impl Into<EngineSource<'a>>,
    message: &str,
    access: Option<&crate::startup_checks::StartupAccessSession>,
    shutdown: Option<&dyn crate::shutdown::host::QuitHandler>,
) -> Option<String> {
    let source = engine.into();
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
                if envelope.name == "app.quit" {
                    return dispatch_quit(envelope, &address, shutdown);
                }
                let closing = shutdown.is_some_and(crate::shutdown::host::QuitHandler::is_closing);
                if closing && !CLOSING_ALLOWED.contains(&envelope.name.as_str()) {
                    return serialize_command_error(envelope, CLOSING_REASON);
                }
                // The engine is read only now, after the quit and closing checks, so a
                // released slot still gets a typed answer.
                let engine = match source.lease() {
                    Ok(engine) => engine,
                    Err(Unavailable::Released) => {
                        return serialize_command_error(envelope, CLOSING_REASON);
                    }
                    Err(Unavailable::Poisoned) => {
                        eprintln!("DiskUnfold: the engine owner lock is poisoned");
                        return serialize_command_error(envelope, "support.internal_error");
                    }
                };
                let caller = CallerCtx::new(Channel::Gui, address.correlation_id, None);
                return call_engine(&engine, envelope, &address.name, &caller, access);
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

fn call_engine(
    engine: &Engine,
    envelope: CallEnvelope,
    name: &str,
    caller: &CallerCtx,
    access: Option<&crate::startup_checks::StartupAccessSession>,
) -> Option<String> {
    if let Some(access) = access {
        if envelope.name == "scan.start"
            && serde_json::from_value::<ScanStartRequest>(envelope.body.clone()).is_ok()
        {
            let _admission = access.lock_analysis_admission();
            if access.refresh() == StartupAccessState::Denied {
                return serialize_analysis_denial(envelope);
            }
            // Keep IF-20 updates out only until the synchronous start is admitted;
            // the scan worker runs after this IPC call returns.
            return respond(engine, envelope, name, caller);
        }
        if envelope.name == "job.resume"
            && serde_json::from_value::<AnalysisJobRequest>(envelope.body.clone()).is_ok()
        {
            // `job.status` is read before the access decision. Keep IF-20 updates
            // serialized across that read, the fresh observation, and the resume
            // admission so a concurrent recheck cannot invalidate the decision.
            let _admission = access.lock_analysis_admission();
            let status_envelope = CallEnvelope {
                name: "job.status".into(),
                contract_version: envelope.contract_version,
                correlation_id: envelope.correlation_id.clone(),
                body: envelope.body.clone(),
            };
            let mut status = engine.call(status_envelope, caller);
            if status.error.is_some() {
                status.name = name.to_owned();
                return serde_json::to_string(&status).ok();
            }
            let Some(kind) = status.body.get("kind").and_then(Value::as_str) else {
                if access.refresh() == StartupAccessState::Denied {
                    return serialize_analysis_denial(envelope);
                }
                return respond(engine, envelope, name, caller);
            };
            let access = access.refresh();
            if access == StartupAccessState::Denied
                && !resume_allowed_without_full_disk_access(kind)
            {
                return serialize_analysis_denial(envelope);
            }
            return respond(engine, envelope, name, caller);
        }
    }
    respond(engine, envelope, name, caller)
}

fn respond(
    engine: &Engine,
    envelope: CallEnvelope,
    name: &str,
    caller: &CallerCtx,
) -> Option<String> {
    let mut response = engine.call(envelope, caller);
    response.name = name.to_owned();
    serde_json::to_string(&response).ok()
}

/// Handles `app.quit` without the engine, so a released engine cannot silence it.
///
/// The envelope version and correlation ID are checked here because this path
/// bypasses the engine pipeline that checks them for every other command.
fn dispatch_quit(
    envelope: CallEnvelope,
    address: &Address,
    shutdown: Option<&dyn crate::shutdown::host::QuitHandler>,
) -> Option<String> {
    if envelope.contract_version != ENVELOPE_VERSION
        || envelope.correlation_id != address.correlation_id
    {
        return serialize_command_error(envelope, "command.envelope_invalid");
    }
    let request = match serde_json::from_value::<AppQuitRequest>(envelope.body.clone()) {
        Ok(request) => request,
        Err(_) => return serialize_command_error(envelope, "command.body_invalid"),
    };
    let Some(shutdown) = shutdown else {
        eprintln!("DiskUnfold shutdown: app.quit arrived without a shutdown controller");
        return serialize_command_error(envelope, "support.internal_error");
    };
    let response: AppQuitResponse = match shutdown.request_quit(&request) {
        Ok(response) => response,
        Err(error) => {
            eprintln!("DiskUnfold shutdown: app.quit failed: {error:?}");
            return serialize_command_error(envelope, "support.internal_error");
        }
    };
    serde_json::to_string(&ResultEnvelope {
        name: envelope.name,
        contract_version: ENVELOPE_VERSION,
        correlation_id: envelope.correlation_id,
        body: serde_json::to_value(response).ok()?,
        error: None,
    })
    .ok()
}

/// Serializes a typed refusal. `startup.shutting_down` is INVALID_ARGUMENT and
/// not retryable, as the F15 HLD allows only INVALID_ARGUMENT and INTERNAL here.
fn serialize_command_error(envelope: CallEnvelope, reason: &str) -> Option<String> {
    let error = if reason == CLOSING_REASON {
        ErrorEnvelope {
            code: ErrorCode::InvalidArgument,
            reason_key: reason.into(),
            message_key: reason.into(),
            correlation_id: envelope.correlation_id.clone(),
            item_refs: Vec::new(),
            retryable: false,
        }
    } else {
        ErrorEnvelope::from_reason_or_internal(reason, envelope.correlation_id.clone(), Vec::new())
    };
    serde_json::to_string(&ResultEnvelope {
        name: envelope.name,
        contract_version: ENVELOPE_VERSION,
        correlation_id: envelope.correlation_id,
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
mod tests;
