//! Closing-state behavior of the dispatcher: body checks, engine-free status, error mapping.
use super::host::{granted_access, host, message, quick};
use super::*;

fn ask(
    slot: &EngineSlot,
    shutdown: &dyn QuitHandler,
    access: &crate::startup_checks::StartupAccessSession,
    name: &str,
    body: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let reply = dispatch_with_shutdown(slot, &message(name, 1, body), access, shutdown)
        .ok_or("the dispatcher gave no reply")?;
    Ok(serde_json::from_str(&reply)?)
}

#[test]
fn quit_rejects_every_body_that_is_not_an_object() -> Result<(), Box<dyn std::error::Error>> {
    let engine = EngineBuilder::default().build()?;
    let shutdown = controller(FakePorts::default());
    for body in [
        json!(["normal"]),
        json!([]),
        json!("normal"),
        json!(7),
        Value::Null,
    ] {
        let reply = dispatch_quit(&engine, &shutdown, body.clone())?;
        assert_eq!(
            reply["error"]["reasonKey"], "command.body_invalid",
            "{body} must not be read as the request struct"
        );
        assert_eq!(reply["error"]["code"], "INVALID_ARGUMENT");
    }
    assert!(!shutdown.is_closing(), "a malformed body admits nothing");
    Ok(())
}

#[test]
fn startup_status_is_served_without_the_engine_while_closing()
-> Result<(), Box<dyn std::error::Error>> {
    let access = Arc::new(granted_access());
    let report = crate::startup_checks::status(false, access.current(), &[])
        .map_err(|error| format!("{error:?}"))?;
    let mut builder = EngineBuilder::default();
    crate::startup_checks::register(&mut builder, report, access.clone());
    let slot: EngineSlot = std::sync::Mutex::new(Some(Arc::new(builder.build()?)));
    let shutdown = controller(FakePorts::default());

    let before = ask(&slot, &shutdown, &access, "app.startup_status", json!({}))?;
    assert!(
        before["error"].is_null(),
        "served by the engine before closing"
    );

    let accepted = ask(
        &slot,
        &shutdown,
        &access,
        "app.quit",
        json!({"mode":"normal"}),
    )?;
    assert_eq!(accepted["body"]["outcome"], "accepted");
    slot.lock().map_err(|_| "engine lock")?.take();

    let during = ask(&slot, &shutdown, &access, "app.startup_status", json!({}))?;
    assert!(
        during["error"].is_null(),
        "no engine is needed while closing"
    );
    assert_eq!(during["body"], before["body"]);
    assert_eq!(during["body"]["access"], "confirmed");
    let refused = ask(
        &slot,
        &shutdown,
        &access,
        "app.startup_status",
        json!({"extra":1}),
    )?;
    assert_eq!(refused["error"]["reasonKey"], "command.body_invalid");
    let other = ask(&slot, &shutdown, &access, "app.product_info", json!({}))?;
    assert_eq!(other["error"]["reasonKey"], "startup.shutting_down");
    Ok(())
}

#[test]
fn startup_status_before_the_report_exists_is_a_typed_closing_refusal()
-> Result<(), Box<dyn std::error::Error>> {
    let access = granted_access();
    let slot: EngineSlot = std::sync::Mutex::new(None);
    let shutdown = controller(FakePorts::default());
    ask(
        &slot,
        &shutdown,
        &access,
        "app.quit",
        json!({"mode":"normal"}),
    )?;
    let reply = ask(&slot, &shutdown, &access, "app.startup_status", json!({}))?;
    assert_eq!(reply["error"]["reasonKey"], "startup.shutting_down");
    assert_eq!(reply["error"]["retryable"], false);
    Ok(())
}

#[test]
fn a_job_shutdown_race_reads_as_permanent_closing_only_once_the_app_is_closing()
-> Result<(), Box<dyn std::error::Error>> {
    let transient = serde_json::to_string(&ResultEnvelope {
        name: "scan.start".into(),
        contract_version: 1,
        correlation_id: Id::parse(CORRELATION)?,
        body: Value::Null,
        error: ErrorEnvelope::from_reason("job.shutting_down", Id::parse(CORRELATION)?, Vec::new()),
    })?;
    let shutdown = controller(FakePorts::default());
    let open: Value = serde_json::from_str(&closing_error_for_job_shutdown(
        transient.clone(),
        Some(&shutdown),
    ))?;
    assert_eq!(open["error"]["reasonKey"], "job.shutting_down");
    assert_eq!(
        open["error"]["retryable"], true,
        "before closing it is transient"
    );

    let engine = EngineBuilder::default().build()?;
    dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    let closing: Value =
        serde_json::from_str(&closing_error_for_job_shutdown(transient, Some(&shutdown)))?;
    assert_eq!(closing["error"]["reasonKey"], "startup.shutting_down");
    assert_eq!(closing["error"]["code"], "INVALID_ARGUMENT");
    assert_eq!(closing["error"]["retryable"], false);
    Ok(())
}

#[test]
fn a_window_close_with_change_work_publishes_a_confirmation_request_without_overloading_the_count()
-> Result<(), Box<dyn std::error::Error>> {
    let job = RunningChangeJob::start(false)?;
    let host = host(&job, quick(ShutdownTimeouts::default()))?;
    host.shutdown.announce_running()?;
    host.shutdown.request_window_close();
    assert!(
        !host.shutdown.is_closing(),
        "the close is not admitted unconfirmed"
    );
    let mut bodies = Vec::new();
    for _ in 0..40 {
        while let Some(wire) = host.events.next_json() {
            let event: du_core_contracts::EventEnvelope = serde_json::from_str(&wire)?;
            if event.topic == "app.lifecycle_changed" {
                bodies.push(event.body);
            }
        }
        if bodies.len() >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let request = bodies.last().ok_or("no lifecycle event")?;
    assert_eq!(request["state"], "running");
    assert_eq!(request["previousState"], "running");
    assert_eq!(request["confirmationRequired"], true);
    assert_eq!(request["inProgressChangeCount"], 1);
    assert_eq!(request["remainingJobCount"], 1, "every remaining job");
    assert!(
        bodies[0].get("confirmationRequired").is_none(),
        "the plain running event stays as published"
    );
    job.release();
    Ok(())
}

#[test]
fn the_host_flushes_finished_job_history_without_waiting_for_active_jobs()
-> Result<(), Box<dyn std::error::Error>> {
    let job = RunningChangeJob::start(true)?;
    let host = host(&job, quick(ShutdownTimeouts::default()))?;
    let status = host
        .shutdown
        .with_ports_mut(|ports| {
            ports.begin_terminal_history_flush(Duration::from_secs(2))?;
            for _ in 0..400 {
                let status = ports.poll_terminal_history_flush();
                if status != HistoryCommitStatus::Pending {
                    return Ok(status);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err("the flush never completed".to_owned())
        })
        .ok_or("missing ports")??;
    assert_eq!(status, HistoryCommitStatus::Complete);
    let counts = host
        .shutdown
        .with_ports_mut(|ports| ports.active_job_counts())
        .ok_or("missing ports")??;
    assert_eq!(
        (counts.total, counts.change),
        (1, 1),
        "the job was not waited for"
    );
    job.release();
    Ok(())
}

#[test]
fn gateway_counts_survive_a_failed_close_attempt() -> Result<(), Box<dyn std::error::Error>> {
    let gate = du_mcp_gateway::access::AccessGate::default();
    let session = gate.connect().map_err(|error| format!("{error:?}"))?;
    let request = session
        .begin_request()
        .map_err(|error| format!("{error:?}"))?;
    session
        .queue_response(&request, vec![1])
        .map_err(|error| format!("{error:?}"))?;
    let mut ports = HostShutdownPorts::new(HostShutdownResources {
        engine: Arc::new(std::sync::Mutex::new(None)),
        gateway: GatewayResources {
            gate: gate.clone(),
            listener: Arc::new(FailingListenerCloser {
                fail_once: AtomicBool::new(true),
                calls: AtomicUsize::new(0),
            }),
        },
        events: Arc::new(DesktopEvents::default()),
        storage: StorageResources {
            database: Arc::new(std::sync::Mutex::new(None)),
            release_extra_owners: Box::new(|| {}),
        },
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    assert!(ports.close_mcp_access(deadline).is_err());
    let report = ports.close_mcp_access(deadline)?;
    assert_eq!(
        (report.sessions_closed, report.responses_discarded),
        (1, 1),
        "the retry finds the gate already closed but keeps the first attempt's counts"
    );
    assert_eq!(ports.gateway_close_counts(), (1, 1));
    Ok(())
}
