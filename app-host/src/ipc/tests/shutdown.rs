use super::*;
use crate::shutdown::{
    GatewayCloseReport, LifecycleEvent, ListenerClose, QuitAdmission, ShutdownError, ShutdownPorts,
    host::{
        GatewayListenerCloser, HostShutdownPorts, HostShutdownResources, QuitHandler,
        ShutdownController,
    },
};
use du_core_contracts::Id;
use du_engine_api::shutdown::{HistoryCommitStatus, ShutdownPreparation};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const CORRELATION: &str = "0195f856-ab83-7a59-9343-10d29775c9a9";

#[derive(Default)]
struct FakePorts {
    active_changes: usize,
    active_jobs: Vec<Id>,
    fail_close_once: bool,
    calls: Vec<&'static str>,
    barrier: VecDeque<HistoryCommitStatus>,
}

impl ShutdownPorts for FakePorts {
    fn close_job_admission(&mut self, confirmed: bool) -> Result<QuitAdmission, String> {
        self.calls.push("admission");
        if self.active_changes > 0 && !confirmed {
            return Ok(QuitAdmission::ConfirmRequired {
                active_change_jobs: self.active_changes,
            });
        }
        Ok(QuitAdmission::Accepted {
            active_change_jobs: self.active_changes,
        })
    }

    fn close_mcp_access(&mut self, _: Instant) -> Result<GatewayCloseReport, String> {
        self.calls.push("gateway");
        if self.fail_close_once {
            self.fail_close_once = false;
            return Err("injected listener close failure".to_owned());
        }
        Ok(GatewayCloseReport {
            access_closed: true,
            listener: ListenerClose::NotInstalled,
            timed_out: false,
            detail: None,
        })
    }

    fn prepare_engine_shutdown(&mut self) -> Result<ShutdownPreparation, String> {
        self.calls.push("prepare");
        Ok(ShutdownPreparation {
            watch_stop: du_engine_api::shutdown::WatchStopResult::NotInstalled,
            cancellations: Vec::new(),
        })
    }

    fn list_active_jobs(&mut self) -> Result<Vec<Id>, String> {
        self.calls.push("list");
        Ok(self.active_jobs.clone())
    }

    fn begin_history_commit_barrier(&mut self, _: Duration) -> Result<(), String> {
        self.calls.push("barrier");
        Ok(())
    }

    fn poll_history_commit_barrier(&mut self) -> HistoryCommitStatus {
        self.calls.push("poll");
        self.barrier
            .pop_front()
            .unwrap_or(HistoryCommitStatus::Complete)
    }

    fn history_failure_count(&self) -> Option<usize> {
        None
    }

    fn close_job_workers(&mut self) -> Result<(), String> {
        self.calls.push("workers");
        Ok(())
    }

    fn release_engine(&mut self) {
        self.calls.push("release_engine");
    }

    fn checkpoint_and_close_database(&mut self) -> Result<(), String> {
        self.calls.push("database");
        Ok(())
    }

    fn zeroize_secrets(&mut self) -> Result<(), String> {
        self.calls.push("zeroize");
        Ok(())
    }

    fn publish_lifecycle(&mut self, _: LifecycleEvent) {}
}

fn controller(ports: FakePorts) -> ShutdownController<FakePorts> {
    ShutdownController::new(ports)
}

fn request(body: Value) -> String {
    json!({"command":"du_call", "args":{"envelope":{
        "name":"app.quit", "contractVersion":1,
        "correlationId":CORRELATION, "body":body,
    }}})
    .to_string()
}

fn dispatch_quit(
    engine: &Engine,
    shutdown: &dyn QuitHandler,
    body: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(
        &dispatch_inner(engine, &request(body), None, Some(shutdown))
            .ok_or("missing app.quit response")?,
    )?)
}

fn pump_to_exit(shutdown: &ShutdownController<FakePorts>) -> Result<(), ShutdownError> {
    for tick in 0..8 {
        shutdown.pump(tick)?;
        if shutdown.should_exit() {
            return Ok(());
        }
    }
    Err(ShutdownError::Engine(
        "fake shutdown did not finish".to_owned(),
    ))
}

struct FailingListenerCloser {
    fail_once: AtomicBool,
    calls: AtomicUsize,
}

impl GatewayListenerCloser for FailingListenerCloser {
    fn close_listener(&self, _: Instant) -> Result<ListenerClose, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_once.swap(false, Ordering::SeqCst) {
            Err("injected listener close failure".to_owned())
        } else {
            Ok(ListenerClose::NotInstalled)
        }
    }
}

#[test]
fn ipc_normal_quit_advances_the_shutdown_pipeline() -> Result<(), Box<dyn std::error::Error>> {
    let engine = EngineBuilder::default().build()?;
    let shutdown = controller(FakePorts::default());
    let reply = dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    assert_eq!(reply["body"]["outcome"], "accepted");
    pump_to_exit(&shutdown)?;
    let calls = shutdown
        .with_ports(|ports| ports.calls.clone())
        .ok_or("missing ports")?;
    assert_eq!(
        calls,
        [
            "admission",
            "gateway",
            "prepare",
            "list",
            "list",
            "barrier",
            "poll",
            "workers",
            "release_engine",
            "database",
            "zeroize"
        ]
    );
    Ok(())
}

#[test]
fn repeated_quit_reports_already_closing_without_repeating_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let engine = EngineBuilder::default().build()?;
    let shutdown = controller(FakePorts::default());
    let first = dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    let second = dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    assert_eq!(first["body"]["outcome"], "accepted");
    assert_eq!(second["body"]["outcome"], "already_closing");
    assert_eq!(
        shutdown
            .with_ports(|ports| ports.calls.clone())
            .ok_or("missing ports")?,
        ["admission"]
    );
    Ok(())
}

#[test]
fn quit_during_active_change_requires_gui_confirmation_then_drains()
-> Result<(), Box<dyn std::error::Error>> {
    let job = Id::parse(CORRELATION)?;
    let engine = EngineBuilder::default().build()?;
    let shutdown = controller(FakePorts {
        active_changes: 1,
        active_jobs: vec![job],
        ..FakePorts::default()
    });
    let refused = dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    assert_eq!(refused["body"]["outcome"], "confirm_required");
    assert_eq!(refused["body"]["inProgressChangeCount"], 1);
    assert!(!shutdown.is_closing());

    let accepted = dispatch_quit(
        &engine,
        &shutdown,
        json!({
            "mode":"normal", "confirmUnfinishedChanges":true,
        }),
    )?;
    assert_eq!(accepted["body"]["outcome"], "accepted");
    shutdown.pump(1)?;
    shutdown.pump(2)?;
    assert_eq!(shutdown.remaining_tasks(), 1);
    shutdown
        .with_ports_mut(|ports| ports.active_jobs.clear())
        .ok_or("missing ports")?;
    pump_to_exit(&shutdown)?;
    Ok(())
}

#[test]
fn listener_close_failure_is_retryable_and_prevents_engine_preparation()
-> Result<(), Box<dyn std::error::Error>> {
    let engine = EngineBuilder::default().build()?;
    let shutdown = controller(FakePorts {
        fail_close_once: true,
        ..FakePorts::default()
    });
    let first = dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    assert_eq!(first["body"]["outcome"], "accepted");
    assert!(matches!(shutdown.pump(1), Err(ShutdownError::Gateway(_))));
    assert!(
        !shutdown
            .with_ports(|ports| ports.calls.contains(&"prepare"))
            .ok_or("missing ports")?
    );
    let repeated = dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    assert_eq!(repeated["body"]["outcome"], "already_closing");
    pump_to_exit(&shutdown)?;
    Ok(())
}

#[test]
fn ipc_listener_port_failure_keeps_access_closed_and_retries_truthfully()
-> Result<(), Box<dyn std::error::Error>> {
    let engine = Arc::new(EngineBuilder::default().build()?);
    let gate = du_mcp_gateway::access::AccessGate::default();
    let listener = Arc::new(FailingListenerCloser {
        fail_once: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    });
    let shutdown = ShutdownController::new(HostShutdownPorts::new(HostShutdownResources {
        engine: Arc::new(std::sync::Mutex::new(Some(engine.clone()))),
        gate: gate.clone(),
        listener: listener.clone(),
        events: Arc::new(du_engine_api::desktop::DesktopEvents::default()),
        database: Arc::new(std::sync::Mutex::new(None)),
        release_extra_owners: Box::new(|| {}),
    }));
    let accepted = dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    assert_eq!(accepted["body"]["outcome"], "accepted");
    assert!(matches!(shutdown.pump(1), Err(ShutdownError::Gateway(_))));
    assert!(!gate.is_accepting());
    assert_eq!(listener.calls.load(Ordering::SeqCst), 1);

    shutdown.pump(2)?;
    assert_eq!(listener.calls.load(Ordering::SeqCst), 2);
    assert!(!shutdown.should_exit());
    Ok(())
}

#[test]
fn ipc_rejects_new_commands_after_shutdown_admission_starts()
-> Result<(), Box<dyn std::error::Error>> {
    let engine = EngineBuilder::default().build()?;
    let shutdown = controller(FakePorts::default());
    dispatch_quit(&engine, &shutdown, json!({"mode":"normal"}))?;
    let command = json!({"command":"du_call", "args":{"envelope":{
        "name":"app.product_info", "contractVersion":1,
        "correlationId":CORRELATION, "body":{},
    }}})
    .to_string();
    let reply: Value = serde_json::from_str(
        &dispatch_inner(&engine, &command, None, Some(&shutdown))
            .ok_or("missing rejected command response")?,
    )?;
    assert_eq!(reply["error"]["reasonKey"], "job.shutting_down");
    Ok(())
}
