use super::*;

fn granted_access() -> crate::startup_checks::StartupAccessSession {
    crate::startup_checks::StartupAccessSession::new(Arc::new(FakeStartupCapabilities::new(
        FullDiskAccessStatus::Granted,
        WebViewAvailability::Unknown,
        PrivacySettingsOpenOutcome::RequestAccepted,
        StartupDialogOutcome::Acknowledged,
    )))
}

fn message(name: &str, version: u32, body: Value) -> String {
    json!({"command":"du_call", "args":{"envelope":{
        "name":name, "contractVersion":version,
        "correlationId":CORRELATION, "body":body,
    }}})
    .to_string()
}

type EngineSlot = std::sync::Mutex<Option<Arc<Engine>>>;
type DatabaseSlot = std::sync::Mutex<Option<crate::shared_database::SharedDatabase>>;

/// The composed host: a real job runtime, access gate, event queue and database.
struct Host {
    slot: Arc<EngineSlot>,
    database: Arc<DatabaseSlot>,
    gate: du_mcp_gateway::access::AccessGate,
    events: Arc<DesktopEvents>,
    shutdown: ShutdownController<HostShutdownPorts>,
    access: crate::startup_checks::StartupAccessSession,
    _data: tempfile::TempDir,
}

fn host(
    job: &RunningChangeJob,
    timeouts: ShutdownTimeouts,
) -> Result<Host, Box<dyn std::error::Error>> {
    let data = tempfile::tempdir()?;
    let slot = Arc::new(std::sync::Mutex::new(Some(job.engine.clone())));
    let database = Arc::new(std::sync::Mutex::new(Some(
        crate::startup_database::open_with_status(data.path())?.database,
    )));
    let gate = du_mcp_gateway::access::AccessGate::default();
    let events = Arc::new(DesktopEvents::default());
    events.begin_document_generation();
    events.mark_document_ready();
    let shutdown = ShutdownController::with_timeouts(
        HostShutdownPorts::new(HostShutdownResources {
            engine: slot.clone(),
            gateway: GatewayResources {
                gate: gate.clone(),
                listener: Arc::new(crate::shutdown::host::NoListenerCloser),
            },
            events: events.clone(),
            storage: StorageResources {
                database: database.clone(),
                release_extra_owners: Box::new(|| {}),
            },
        }),
        timeouts,
    );
    Ok(Host {
        slot,
        database,
        gate,
        events,
        shutdown,
        access: granted_access(),
        _data: data,
    })
}

fn call(host: &Host, name: &str, body: Value) -> Result<Value, Box<dyn std::error::Error>> {
    let reply = dispatch_with_shutdown(
        host.slot.as_ref(),
        &message(name, 1, body),
        &host.access,
        &host.shutdown,
    )
    .ok_or("the dispatcher gave no reply")?;
    Ok(serde_json::from_str(&reply)?)
}

fn pump_until_exit(host: &Host) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..600 {
        // Failed steps are logged and counted by the coordinator.
        let _ = host.shutdown.pump(crate::shutdown::host::current_time_ms());
        if host.shutdown.should_exit() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("the shutdown did not finish".into())
}

/// Lifecycle states and remaining counts in publication order.
fn lifecycle(host: &Host) -> Vec<(String, u64, u64)> {
    let mut seen = Vec::new();
    for _ in 0..40 {
        while let Some(wire) = host.events.next_json() {
            let Ok(event) = serde_json::from_str::<du_core_contracts::EventEnvelope>(&wire) else {
                continue;
            };
            if event.topic == "app.lifecycle_changed" {
                seen.push((
                    event.body["state"].as_str().unwrap_or_default().to_owned(),
                    event.body["stateSequence"].as_u64().unwrap_or_default(),
                    event.body["remainingJobCount"].as_u64().unwrap_or_default(),
                ));
            }
        }
        if seen.iter().any(|(state, ..)| state == "closed") {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    seen
}

fn quick(timeouts: ShutdownTimeouts) -> ShutdownTimeouts {
    ShutdownTimeouts {
        final_flush: Duration::from_millis(200),
        ..timeouts
    }
}

#[test]
fn composed_host_cancels_a_running_change_job_records_its_history_and_closes_the_database()
-> Result<(), Box<dyn std::error::Error>> {
    let job = RunningChangeJob::start(false)?;
    let host = host(&job, quick(ShutdownTimeouts::default()))?;
    host.shutdown.announce_running()?;

    let refused = call(&host, "app.quit", json!({"mode":"normal"}))?;
    assert_eq!(refused["body"]["outcome"], "confirm_required");
    assert_eq!(refused["body"]["inProgressChangeCount"], 1);
    let status = call(&host, "job.status", json!({"jobRef": job.job_ref}))?;
    assert_eq!(
        status["body"]["state"], "running",
        "an unconfirmed quit leaves the job alone"
    );

    // A window close is a normal exit; with change work running it asks the window
    // to confirm instead of being ignored, and it does not start the shutdown.
    host.shutdown.request_window_close();
    assert!(!host.shutdown.is_closing());

    let accepted = call(
        &host,
        "app.quit",
        json!({"mode":"normal","confirmUnfinishedChanges":true}),
    )?;
    assert_eq!(accepted["body"]["outcome"], "accepted");
    let rejected = call(&host, "app.product_info", json!({}))?;
    assert_eq!(rejected["error"]["reasonKey"], "startup.shutting_down");
    assert_eq!(rejected["error"]["retryable"], false);
    let listed = call(&host, "job.list", json!({}))?;
    assert_ne!(listed["error"]["reasonKey"], "startup.shutting_down");
    assert_eq!(
        call(&host, "app.quit", json!({"mode":"normal"}))?["body"]["outcome"],
        "already_closing"
    );

    pump_until_exit(&host)?;
    host.shutdown.exit_result()?;
    assert_eq!(
        job.recorded_history(),
        [du_core_contracts::job::JobState::Cancelled]
    );
    assert!(
        host.database.lock().map_err(|_| "database lock")?.is_none(),
        "database closed"
    );
    assert!(
        host.slot.lock().map_err(|_| "engine lock")?.is_none(),
        "engine released"
    );
    assert_eq!(
        job.window_sessions_retired(),
        1,
        "window session retired on the orderly path"
    );
    assert!(!host.gate.is_accepting());

    let late = call(&host, "job.status", json!({"jobRef": job.job_ref}))?;
    assert_eq!(
        late["error"]["reasonKey"], "startup.shutting_down",
        "typed reply after release"
    );

    assert_lifecycle_story(&lifecycle(&host));
    Ok(())
}

fn assert_lifecycle_story(states: &[(String, u64, u64)]) {
    let names: Vec<_> = states.iter().map(|(state, ..)| state.as_str()).collect();
    assert_eq!(names.first(), Some(&"running"));
    assert_eq!(names.last(), Some(&"closed"));
    let access_closed = states.iter().find(|(state, ..)| state == "access_closed");
    assert_eq!(access_closed.map(|(.., remaining)| *remaining), Some(1));
    assert!(
        states.windows(2).all(|pair| pair[0].1 < pair[1].1),
        "sequence is monotonic"
    );
    let running = names.iter().filter(|state| **state == "running").count();
    assert!(
        running >= 2,
        "the confirmation notice is published as a running event"
    );
}

#[test]
fn without_waiting_cancels_then_abandons_a_job_stuck_in_an_os_call_without_a_history_record()
-> Result<(), Box<dyn std::error::Error>> {
    let job = RunningChangeJob::start(true)?;
    let timeouts = ShutdownTimeouts {
        os_return: Duration::from_millis(60),
        ..ShutdownTimeouts::default()
    };
    let host = host(&job, quick(timeouts))?;
    host.shutdown.announce_running()?;

    let refused = call(&host, "app.quit", json!({"mode":"without_waiting"}))?;
    assert_eq!(refused["body"]["outcome"], "confirm_required");
    assert!(!host.shutdown.is_closing());
    let accepted = call(
        &host,
        "app.quit",
        json!({"mode":"without_waiting","confirmUnfinishedChanges":true}),
    )?;
    assert_eq!(accepted["body"]["outcome"], "accepted");

    pump_until_exit(&host)?;
    host.shutdown.exit_result()?;
    assert!(
        job.recorded_history().is_empty(),
        "the unfinished job is never recorded"
    );
    assert!(
        host.database.lock().map_err(|_| "database lock")?.is_some(),
        "storage stays for recovery"
    );
    assert!(host.slot.lock().map_err(|_| "engine lock")?.is_none());
    let states = lifecycle(&host);
    assert_eq!(
        states
            .last()
            .map(|(state, _, remaining)| (state.as_str(), *remaining)),
        Some(("closed", 1))
    );
    job.release();
    Ok(())
}

#[test]
fn asking_to_leave_without_waiting_during_a_drain_switches_to_the_short_bound()
-> Result<(), Box<dyn std::error::Error>> {
    let job = RunningChangeJob::start(true)?;
    let timeouts = ShutdownTimeouts {
        os_return: Duration::from_millis(60),
        ..ShutdownTimeouts::default()
    };
    let host = host(&job, quick(timeouts))?;
    let body = json!({"mode":"normal","confirmUnfinishedChanges":true});
    assert_eq!(
        call(&host, "app.quit", body)?["body"]["outcome"],
        "accepted"
    );
    for _ in 0..3 {
        let _ = host.shutdown.pump(crate::shutdown::host::current_time_ms());
    }
    assert!(
        !host.shutdown.should_exit(),
        "a waiting drain keeps waiting on the stuck job"
    );
    let switched = call(&host, "app.quit", json!({"mode":"without_waiting"}))?;
    assert_eq!(switched["body"]["outcome"], "already_closing");
    pump_until_exit(&host)?;
    host.shutdown.exit_result()?;
    assert!(job.recorded_history().is_empty());
    job.release();
    Ok(())
}

#[test]
fn a_waiting_shutdown_with_a_stuck_job_hits_the_drain_bound_and_exits_nonzero()
-> Result<(), Box<dyn std::error::Error>> {
    let job = RunningChangeJob::start(true)?;
    let timeouts = ShutdownTimeouts {
        drain: Duration::from_millis(60),
        ..ShutdownTimeouts::default()
    };
    let host = host(&job, quick(timeouts))?;
    let body = json!({"mode":"normal","confirmUnfinishedChanges":true});
    assert_eq!(
        call(&host, "app.quit", body)?["body"]["outcome"],
        "accepted"
    );
    pump_until_exit(&host)?;
    assert_eq!(
        host.shutdown.exit_result(),
        Err(ShutdownError::DrainTimedOut { remaining: 1 })
    );
    job.release();
    Ok(())
}

#[test]
fn a_step_that_never_succeeds_forces_a_nonzero_exit_after_bounded_retries()
-> Result<(), Box<dyn std::error::Error>> {
    let engine = EngineBuilder::default().build()?;
    let timeouts = ShutdownTimeouts {
        phase_retries: 4,
        final_flush: Duration::ZERO,
        ..ShutdownTimeouts::default()
    };
    let shutdown = ShutdownController::with_timeouts(
        FakePorts {
            database_always_fails: true,
            ..FakePorts::default()
        },
        timeouts,
    );
    assert_eq!(
        dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?["body"]["outcome"],
        "accepted"
    );
    let mut errors = 0;
    for tick in 0..40 {
        if shutdown.pump(tick).is_err() {
            errors += 1;
        }
        if shutdown.should_exit() {
            break;
        }
    }
    assert!(shutdown.should_exit());
    assert_eq!(errors, 4, "retries stop at the bound");
    assert!(matches!(
        shutdown.exit_result(),
        Err(ShutdownError::RetriesExhausted { .. })
    ));
    let finished = shutdown.with_ports(|ports| {
        ports
            .calls
            .iter()
            .filter(|call| **call == "database")
            .count()
    });
    assert_eq!(finished, Some(4));
    Ok(())
}

#[test]
fn a_poisoned_coordinator_lock_is_a_forced_exit_with_a_typed_quit_reply()
-> Result<(), Box<dyn std::error::Error>> {
    let engine = EngineBuilder::default().build()?;
    let shutdown = controller(FakePorts::default());
    shutdown.poison_for_test();
    assert!(shutdown.should_exit());
    assert!(shutdown.is_closing());
    assert_eq!(shutdown.exit_result(), Err(ShutdownError::Poisoned));
    assert_eq!(shutdown.pump(1), Err(ShutdownError::Poisoned));
    let reply = dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    assert_eq!(reply["error"]["reasonKey"], "support.internal_error");
    Ok(())
}

#[test]
fn quit_validates_the_envelope_and_always_replies_even_without_an_engine()
-> Result<(), Box<dyn std::error::Error>> {
    let shutdown = controller(FakePorts::default());
    let slot: EngineSlot = std::sync::Mutex::new(None);
    let access = granted_access();
    let ask =
        |version: u32, name: &str, body: Value| -> Result<Value, Box<dyn std::error::Error>> {
            let reply =
                dispatch_with_shutdown(&slot, &message(name, version, body), &access, &shutdown)
                    .ok_or("no reply")?;
            Ok(serde_json::from_str(&reply)?)
        };
    let stale = ask(2, "app.quit", json!({"mode":"normal"}))?;
    assert_eq!(stale["error"]["reasonKey"], "command.envelope_invalid");
    assert!(!shutdown.is_closing(), "an invalid envelope admits nothing");
    let before = ask(1, "job.status", json!({"jobRef": CORRELATION}))?;
    assert!(
        before["error"]["reasonKey"].is_string(),
        "typed reply for an empty slot"
    );
    let accepted = ask(1, "app.quit", json!({"mode":"normal"}))?;
    assert_eq!(accepted["body"]["outcome"], "accepted");
    let after = ask(1, "job.cancel", json!({"jobRef": CORRELATION}))?;
    assert_eq!(after["error"]["reasonKey"], "startup.shutting_down");
    assert_eq!(after["error"]["retryable"], false);
    Ok(())
}

struct SlowListener(Duration);

impl GatewayListenerCloser for SlowListener {
    fn close_listener(&self, _: Instant) -> Result<ListenerClose, String> {
        std::thread::sleep(self.0);
        Ok(ListenerClose::NotInstalled)
    }
}

#[test]
fn gateway_close_reports_if12_counts_and_a_truthful_timeout()
-> Result<(), Box<dyn std::error::Error>> {
    for (delay, deadline, timed_out) in [(0, 5_000, false), (40, 5, true)] {
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
                listener: Arc::new(SlowListener(Duration::from_millis(delay))),
            },
            events: Arc::new(DesktopEvents::default()),
            storage: StorageResources {
                database: Arc::new(std::sync::Mutex::new(None)),
                release_extra_owners: Box::new(|| {}),
            },
        });
        let report = ports.close_mcp_access(Instant::now() + Duration::from_millis(deadline))?;
        assert!(report.access_closed && !gate.is_accepting());
        assert_eq!(report.timed_out, timed_out);
        assert_eq!((report.sessions_closed, report.responses_discarded), (1, 1));
        assert_eq!(report.listener, ListenerClose::NotInstalled);
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn quit_is_routed_through_a_control_lane_not_the_ordered_lane() {
    use du_platform_os::webview::DispatchLane;
    let route = |name: &str| dispatch_lane(&message(name, 1, json!({})));
    assert_eq!(route("app.quit"), DispatchLane::JobControl);
    assert_eq!(route("job.cancel"), DispatchLane::JobControl);
    assert_eq!(route("app.startup_status"), DispatchLane::Ordered);
}
