use super::*;
use std::collections::{HashSet, VecDeque};

#[derive(Default)]
struct FakePorts {
    admission: Option<QuitAdmission>,
    gateway_reports: VecDeque<GatewayCloseReport>,
    active: HashSet<Id>,
    barrier_statuses: VecDeque<HistoryCommitStatus>,
    history_failures: Option<usize>,
    watch_stop: Option<du_engine_api::shutdown::WatchStopResult>,
    preparation: Option<ShutdownPreparation>,
    calls: Vec<&'static str>,
    events: Vec<LifecycleEvent>,
    fail_database_once: bool,
    db_closed: bool,
    secrets_cleared: bool,
    released: bool,
    workers_closed: bool,
}

impl ShutdownPorts for FakePorts {
    fn close_job_admission(&mut self, _: bool) -> Result<QuitAdmission, String> {
        self.calls.push("admission");
        Ok(self.admission.unwrap_or(QuitAdmission::Accepted {
            active_change_jobs: 0,
        }))
    }

    fn close_mcp_access(&mut self, _: Instant) -> Result<GatewayCloseReport, String> {
        self.calls.push("gateway");
        self.gateway_reports
            .pop_front()
            .ok_or_else(|| "gateway report missing".to_owned())
    }

    fn prepare_engine_shutdown(&mut self) -> Result<ShutdownPreparation, String> {
        self.calls.push("prepare");
        if let Some(preparation) = self.preparation.take() {
            return Ok(preparation);
        }
        Ok(ShutdownPreparation {
            watch_stop: self
                .watch_stop
                .unwrap_or(du_engine_api::shutdown::WatchStopResult::NotInstalled),
            cancellations: Vec::new(),
        })
    }

    fn list_active_jobs(&mut self) -> Result<Vec<Id>, String> {
        self.calls.push("list");
        Ok(self.active.iter().cloned().collect())
    }

    fn begin_history_commit_barrier(&mut self, _: Duration) -> Result<(), String> {
        self.calls.push("history_barrier");
        Ok(())
    }

    fn poll_history_commit_barrier(&mut self) -> HistoryCommitStatus {
        self.calls.push("poll_history");
        self.barrier_statuses
            .pop_front()
            .unwrap_or(HistoryCommitStatus::Complete)
    }

    fn history_failure_count(&self) -> Option<usize> {
        self.history_failures
    }

    fn close_job_workers(&mut self) -> Result<(), String> {
        self.calls.push("workers");
        self.workers_closed = true;
        Ok(())
    }

    fn release_engine(&mut self) {
        self.calls.push("release_engine");
        self.released = true;
    }

    fn checkpoint_and_close_database(&mut self) -> Result<(), String> {
        self.calls.push("database");
        if self.fail_database_once {
            self.fail_database_once = false;
            return Err("database still has a live owner".into());
        }
        self.db_closed = true;
        Ok(())
    }

    fn zeroize_secrets(&mut self) -> Result<(), String> {
        self.calls.push("zeroize");
        self.secrets_cleared = true;
        Ok(())
    }

    fn publish_lifecycle(&mut self, event: LifecycleEvent) {
        self.events.push(event);
    }
}

#[test]
fn cancellation_failure_keeps_the_job_reference_and_active_owner_visible()
-> Result<(), Box<dyn std::error::Error>> {
    let job = id(5)?;
    let preparation = ShutdownPreparation {
        watch_stop: du_engine_api::shutdown::WatchStopResult::NotInstalled,
        cancellations: vec![du_engine_api::shutdown::ShutdownJobCancellation {
            job_ref: job.clone(),
            outcome: Err(du_engine_api::shutdown::RuntimeError::DeliveryBusy),
        }],
    };
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        active: HashSet::from([job.clone()]),
        preparation: Some(preparation.clone()),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);

    coordinator.request_waiting_quit(true)?;
    coordinator.advance(1)?;
    coordinator.advance(2)?;

    assert_eq!(coordinator.preparation(), Some(&preparation));
    assert_eq!(coordinator.remaining_tasks(), 1);
    coordinator.job_finished(&job, 3)?;
    assert_eq!(coordinator.remaining_tasks(), 0);
    Ok(())
}

#[test]
fn unconfirmed_watcher_stop_blocks_job_drain_and_storage_close()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        watch_stop: Some(du_engine_api::shutdown::WatchStopResult::Failed),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);
    coordinator.request_waiting_quit(true)?;
    coordinator.advance(1)?;
    assert!(matches!(
        coordinator.advance(2),
        Err(ShutdownError::WatcherStopUnconfirmed)
    ));
    assert_eq!(coordinator.phase, Phase::AccessClosed);
    assert!(!coordinator.ports.calls.contains(&"list"));
    assert!(!coordinator.ports.calls.contains(&"history_barrier"));
    assert!(!coordinator.ports.workers_closed);
    assert!(!coordinator.ports.db_closed);
    assert!(!coordinator.is_closed());
    Ok(())
}

fn closed_gateway(timed_out: bool) -> GatewayCloseReport {
    GatewayCloseReport {
        access_closed: true,
        listener: ListenerClose::Closed,
        timed_out,
        detail: timed_out.then(|| "graceful close deadline elapsed".into()),
    }
}

fn id(byte: u8) -> Result<Id, Box<dyn std::error::Error>> {
    Id::parse(&format!("00000000-0000-7000-8000-{:012x}", byte))
        .map_err(std::io::Error::other)
        .map_err(Into::into)
}

#[test]
fn confirmation_refusal_leaves_the_entire_shutdown_state_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        admission: Some(QuitAdmission::ConfirmRequired {
            active_change_jobs: 2,
        }),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);

    assert_eq!(
        coordinator.request_waiting_quit(false)?,
        QuitAdmission::ConfirmRequired {
            active_change_jobs: 2
        }
    );
    assert_eq!(coordinator.phase, Phase::Running);
    assert_eq!(coordinator.remaining_tasks(), 0);
    let ports = coordinator.into_ports();
    assert_eq!(ports.calls, ["admission"]);
    assert!(ports.events.is_empty());
    Ok(())
}

#[test]
fn normal_shutdown_closes_gateway_then_drains_history_then_drops_owners()
-> Result<(), Box<dyn std::error::Error>> {
    let job = id(1)?;
    let ports = FakePorts {
        admission: Some(QuitAdmission::Accepted {
            active_change_jobs: 1,
        }),
        active: HashSet::from([job.clone()]),
        gateway_reports: VecDeque::from([closed_gateway(true)]),
        barrier_statuses: VecDeque::from([
            HistoryCommitStatus::Pending,
            HistoryCommitStatus::Complete,
        ]),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);

    assert_eq!(
        coordinator.request_waiting_quit(false)?,
        QuitAdmission::Accepted {
            active_change_jobs: 1
        }
    );
    coordinator.advance(100)?;
    assert_eq!(coordinator.phase, Phase::AccessClosed);
    coordinator.advance(101)?;
    assert_eq!(coordinator.phase, Phase::Draining);
    assert_eq!(coordinator.remaining_tasks(), 1);
    assert_eq!(
        coordinator
            .ports
            .calls
            .iter()
            .take(3)
            .copied()
            .collect::<Vec<_>>(),
        ["admission", "gateway", "prepare"]
    );

    coordinator.job_finished(&job, 102)?;
    coordinator.job_finished(&job, 103)?;
    assert_eq!(coordinator.remaining_tasks(), 0);
    coordinator.advance(104)?;
    assert_eq!(coordinator.phase, Phase::HistoryCommit);
    coordinator.advance(105)?;
    assert_eq!(coordinator.phase, Phase::HistoryCommit);
    coordinator.advance(106)?;
    assert_eq!(coordinator.phase, Phase::ClosingStorage);
    assert!(coordinator.ports.workers_closed);
    assert!(coordinator.ports.released);
    coordinator.advance(107)?;
    assert_eq!(coordinator.phase, Phase::ZeroizingSecrets);
    assert!(coordinator.ports.db_closed);
    coordinator.advance(108)?;
    assert!(coordinator.is_closed());
    assert!(coordinator.ports.secrets_cleared);

    let ports = coordinator.into_ports();
    assert_shutdown_call_order(&ports.calls)?;
    assert_eq!(
        ports
            .events
            .iter()
            .map(|event| event.status)
            .collect::<Vec<_>>(),
        [
            LifecycleStatus::AccessClosed,
            LifecycleStatus::Draining,
            LifecycleStatus::Draining,
            LifecycleStatus::Closed,
        ]
    );
    assert!(
        ports
            .events
            .windows(2)
            .all(|events| events[0].sequence < events[1].sequence)
    );
    assert_eq!(ports.events[2].remaining_tasks, 0);
    Ok(())
}

fn assert_shutdown_call_order(calls: &[&'static str]) -> Result<(), &'static str> {
    let positions = [
        "gateway",
        "prepare",
        "history_barrier",
        "workers",
        "release_engine",
        "database",
        "zeroize",
    ]
    .map(|expected| {
        calls
            .iter()
            .position(|call| *call == expected)
            .ok_or(expected)
    });
    let [
        gateway,
        prepare,
        history,
        workers,
        engine,
        database,
        secrets,
    ] = positions;
    let ordered = [
        gateway?, prepare?, history?, workers?, engine?, database?, secrets?,
    ];
    if ordered.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("shutdown calls are out of order");
    }
    Ok(())
}

#[test]
fn an_unconfirmed_gateway_close_never_starts_bulk_cancellation()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_reports: VecDeque::from([
            GatewayCloseReport {
                access_closed: false,
                listener: ListenerClose::NotInstalled,
                timed_out: true,
                detail: Some("force close failed".into()),
            },
            closed_gateway(true),
        ]),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);
    assert_eq!(
        coordinator.request_waiting_quit(true)?,
        QuitAdmission::Accepted {
            active_change_jobs: 0
        }
    );
    assert!(matches!(
        coordinator.advance(1),
        Err(ShutdownError::GatewayAccessOpen(_))
    ));
    assert_eq!(coordinator.phase, Phase::ClosingAccess);
    assert!(!coordinator.ports.calls.contains(&"prepare"));
    assert_eq!(
        coordinator.request_waiting_quit(true)?,
        QuitAdmission::AlreadyClosing
    );
    coordinator.advance(2)?;
    assert_eq!(coordinator.phase, Phase::AccessClosed);
    coordinator.advance(3)?;
    assert!(coordinator.ports.calls.contains(&"prepare"));
    Ok(())
}

#[test]
fn failed_history_barrier_stays_failed_and_keeps_storage_open()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        barrier_statuses: VecDeque::from([
            HistoryCommitStatus::Failed,
            HistoryCommitStatus::Failed,
        ]),
        history_failures: Some(1),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);
    coordinator.request_waiting_quit(true)?;
    coordinator.advance(1)?;
    coordinator.advance(2)?;
    coordinator.advance(3)?;
    assert!(matches!(
        coordinator.advance(4),
        Err(ShutdownError::HistoryCommitFailed {
            failed_appends: Some(1)
        })
    ));
    assert!(!coordinator.ports.workers_closed);
    assert!(!coordinator.ports.db_closed);
    assert!(!coordinator.is_closed());
    assert!(matches!(
        coordinator.advance(5),
        Err(ShutdownError::HistoryCommitFailed {
            failed_appends: Some(1)
        })
    ));
    assert!(!coordinator.ports.workers_closed);
    assert!(!coordinator.ports.db_closed);
    assert!(!coordinator.is_closed());
    Ok(())
}

#[test]
fn database_close_failure_keeps_handle_available_for_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        barrier_statuses: VecDeque::from([HistoryCommitStatus::Complete]),
        fail_database_once: true,
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);
    coordinator.request_waiting_quit(true)?;
    coordinator.advance(1)?;
    coordinator.advance(2)?;
    coordinator.advance(3)?;
    coordinator.advance(4)?;
    assert_eq!(coordinator.phase, Phase::ClosingStorage);
    assert!(matches!(
        coordinator.advance(5),
        Err(ShutdownError::DatabaseClose(_))
    ));
    assert!(!coordinator.is_closed());
    assert!(!coordinator.ports.secrets_cleared);
    coordinator.advance(6)?;
    coordinator.advance(7)?;
    assert!(coordinator.is_closed());
    Ok(())
}

#[test]
fn history_timeout_is_reported_without_closing_owners_and_later_completion_recovers()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        barrier_statuses: VecDeque::from([
            HistoryCommitStatus::TimedOut,
            HistoryCommitStatus::Complete,
        ]),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);
    coordinator.request_waiting_quit(true)?;
    coordinator.advance(1)?;
    coordinator.advance(2)?;
    coordinator.advance(3)?;
    assert!(matches!(
        coordinator.advance(4),
        Err(ShutdownError::HistoryCommitTimedOut)
    ));
    assert_eq!(coordinator.phase, Phase::HistoryCommit);
    assert!(!coordinator.ports.workers_closed);
    assert!(!coordinator.ports.released);
    assert!(!coordinator.ports.db_closed);
    assert!(!coordinator.is_closed());

    coordinator.advance(5)?;
    assert!(coordinator.ports.workers_closed);
    assert!(coordinator.ports.released);
    assert_eq!(coordinator.phase, Phase::ClosingStorage);
    Ok(())
}

#[test]
fn resync_replaces_the_event_count_with_the_job_list_cut() -> Result<(), Box<dyn std::error::Error>>
{
    let first = id(2)?;
    let second = id(3)?;
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        active: HashSet::from([first.clone(), second.clone()]),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);
    coordinator.request_waiting_quit(true)?;
    coordinator.advance(1)?;
    coordinator.advance(2)?;
    coordinator.job_finished(&first, 3)?;
    assert_eq!(coordinator.remaining_tasks(), 1);
    coordinator.ports.active = HashSet::from([second.clone()]);
    coordinator.resync_active_jobs(4)?;
    assert_eq!(coordinator.remaining_tasks(), 1);
    coordinator.ports.active.clear();
    coordinator.resync_active_jobs(5)?;
    assert_eq!(coordinator.remaining_tasks(), 0);
    coordinator.advance(6)?;
    assert_eq!(coordinator.phase, Phase::HistoryCommit);
    Ok(())
}
