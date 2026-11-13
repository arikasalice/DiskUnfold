use super::*;

fn drain_ready_coordinator(
    ports: FakePorts,
    timeouts: ShutdownTimeouts,
    mode: QuitMode,
) -> Result<ShutdownCoordinator<FakePorts>, Box<dyn std::error::Error>> {
    let mut coordinator = ShutdownCoordinator::with_timeouts(ports, timeouts);
    coordinator.request_quit(mode, true)?;
    coordinator.advance(1)?;
    coordinator.advance(2)?;
    assert_eq!(coordinator.phase, Phase::Draining);
    Ok(coordinator)
}

#[test]
fn running_is_announced_first_and_access_closed_carries_the_active_count()
-> Result<(), Box<dyn std::error::Error>> {
    let job = id(7)?;
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        active: HashSet::from([job]),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);
    coordinator.announce_running(10)?;
    coordinator.announce_running(11)?;
    coordinator.request_waiting_quit(true)?;
    coordinator.advance(12)?;
    let ports = coordinator.into_ports();
    assert_eq!(ports.events.len(), 2, "running is announced once");
    assert_eq!(ports.events[0].status, LifecycleStatus::Running);
    assert_eq!(ports.events[0].previous, LifecycleStatus::Starting);
    assert_eq!(ports.events[1].status, LifecycleStatus::AccessClosed);
    assert_eq!(ports.events[1].previous, LifecycleStatus::Running);
    assert_eq!(ports.events[1].remaining_tasks, 1);
    assert!(ports.events[0].sequence < ports.events[1].sequence);
    Ok(())
}

#[test]
fn gateway_report_keeps_the_if12_counts_and_timeout_flag() -> Result<(), Box<dyn std::error::Error>>
{
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(true)]),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);
    coordinator.request_waiting_quit(true)?;
    coordinator.advance(1)?;
    let report = coordinator
        .gateway_report()
        .ok_or("missing gateway report")?;
    assert_eq!((report.sessions_closed, report.responses_discarded), (2, 3));
    assert!(report.timed_out);
    assert!(
        coordinator
            .diagnostics()
            .iter()
            .any(|note| note.contains("deadline"))
    );
    Ok(())
}

#[test]
fn a_gateway_that_keeps_failing_is_force_closed_and_shutdown_moves_on()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_failures_left: 3,
        force_gate: true,
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::with_timeouts(ports, FAST);
    coordinator.request_waiting_quit(true)?;
    assert!(matches!(
        coordinator.advance(1),
        Err(ShutdownError::Gateway(_))
    ));
    assert!(matches!(
        coordinator.advance(2),
        Err(ShutdownError::Gateway(_))
    ));
    coordinator.advance(3)?;
    assert_eq!(coordinator.phase, Phase::AccessClosed);
    let report = coordinator.gateway_report().ok_or("missing report")?;
    assert_eq!(report.listener, ListenerClose::Unconfirmed);
    assert!(report.timed_out);
    Ok(())
}

#[test]
fn a_gateway_that_cannot_be_force_closed_never_starts_cancellation()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_failures_left: 9,
        force_gate: false,
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::with_timeouts(ports, FAST);
    coordinator.request_waiting_quit(true)?;
    for tick in 1..=4 {
        assert!(coordinator.advance(tick).is_err());
    }
    assert_eq!(coordinator.phase, Phase::ClosingAccess);
    assert!(!coordinator.ports.calls.contains(&"prepare"));
    Ok(())
}

#[test]
fn without_waiting_abandons_running_jobs_after_the_os_call_bound_without_history()
-> Result<(), Box<dyn std::error::Error>> {
    let stuck = id(8)?;
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        active: HashSet::from([stuck]),
        ..FakePorts::default()
    };
    let timeouts = ShutdownTimeouts {
        os_return: Duration::ZERO,
        ..FAST
    };
    let mut coordinator = drain_ready_coordinator(ports, timeouts, QuitMode::WithoutWaiting)?;
    coordinator.advance(3)?;
    assert_eq!(coordinator.phase, Phase::ZeroizingSecrets);
    coordinator.advance(4)?;
    assert!(coordinator.is_closed());
    assert_eq!(
        coordinator.outcome(),
        &ShutdownOutcome::Abandoned { jobs: 1 }
    );
    let ports = coordinator.into_ports();
    for skipped in ["history_barrier", "workers", "database"] {
        assert!(
            !ports.calls.contains(&skipped),
            "{skipped} must not run with a job still running"
        );
    }
    assert!(ports.released && ports.secrets_cleared);
    let closed = ports.events.last().ok_or("missing closed event")?;
    assert_eq!(closed.status, LifecycleStatus::Closed);
    assert_eq!(
        closed.remaining_tasks, 1,
        "the abandoned job is reported, not hidden"
    );
    Ok(())
}

#[test]
fn asking_to_leave_without_waiting_shortens_a_waiting_drain()
-> Result<(), Box<dyn std::error::Error>> {
    let stuck = id(9)?;
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        active: HashSet::from([stuck]),
        ..FakePorts::default()
    };
    let timeouts = ShutdownTimeouts {
        os_return: Duration::ZERO,
        ..FAST
    };
    let mut coordinator = drain_ready_coordinator(ports, timeouts, QuitMode::Waiting)?;
    coordinator.advance(3)?;
    assert_eq!(
        coordinator.phase,
        Phase::Draining,
        "a waiting drain keeps waiting"
    );
    assert_eq!(
        coordinator.request_quit(QuitMode::WithoutWaiting, false)?,
        QuitAdmission::AlreadyClosing
    );
    coordinator.advance(4)?;
    assert_eq!(coordinator.phase, Phase::ZeroizingSecrets);
    assert_eq!(
        coordinator.outcome(),
        &ShutdownOutcome::Abandoned { jobs: 1 }
    );
    Ok(())
}

#[test]
fn a_waiting_drain_is_bounded_and_ends_as_a_failure() -> Result<(), Box<dyn std::error::Error>> {
    let stuck = id(10)?;
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        active: HashSet::from([stuck]),
        ..FakePorts::default()
    };
    let timeouts = ShutdownTimeouts {
        drain: Duration::ZERO,
        ..FAST
    };
    let mut coordinator = drain_ready_coordinator(ports, timeouts, QuitMode::Waiting)?;
    coordinator.advance(3)?;
    coordinator.advance(4)?;
    assert!(coordinator.is_closed());
    assert_eq!(
        coordinator.outcome(),
        &ShutdownOutcome::DrainTimedOut { jobs: 1 }
    );
    Ok(())
}

#[test]
fn a_step_that_keeps_failing_forces_the_exit_and_publishes_the_terminal_event()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        database_always_fails: true,
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::with_timeouts(ports, FAST);
    coordinator.request_waiting_quit(true)?;
    let mut results = Vec::new();
    for tick in 1..=12 {
        results.push(coordinator.step(tick));
        if coordinator.is_closed() {
            break;
        }
    }
    assert!(coordinator.is_closed());
    assert!(matches!(
        results.last(),
        Some(Err(ShutdownError::RetriesExhausted {
            phase: "closing_storage",
            ..
        }))
    ));
    assert!(matches!(coordinator.outcome(), ShutdownOutcome::Forced(_)));
    assert!(
        coordinator.diagnostics().len() >= 3,
        "every failed attempt is logged"
    );
    let ports = coordinator.into_ports();
    assert_eq!(
        ports.events.last().map(|event| event.status),
        Some(LifecycleStatus::Closed)
    );
    assert!(!ports.secrets_cleared);
    Ok(())
}

#[test]
fn the_final_event_must_leave_the_queue_before_the_loop_may_exit()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        hold_events: true,
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::with_timeouts(ports, FAST);
    coordinator.request_waiting_quit(true)?;
    for tick in 1..=10 {
        coordinator.step(tick)?;
    }
    assert!(coordinator.is_closed());
    assert!(!coordinator.may_exit(), "queued closed event still pending");
    coordinator.ports.hold_events = false;
    assert!(coordinator.may_exit());

    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        hold_events: true,
        ..FakePorts::default()
    };
    let timeouts = ShutdownTimeouts {
        final_flush: Duration::ZERO,
        ..FAST
    };
    let mut coordinator = ShutdownCoordinator::with_timeouts(ports, timeouts);
    coordinator.request_waiting_quit(true)?;
    for tick in 1..=10 {
        coordinator.step(tick)?;
    }
    assert!(
        coordinator.may_exit(),
        "a missing page cannot hold the exit forever"
    );
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
