use super::*;
use du_engine_api::shutdown::ActiveJobCounts;

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
        &ShutdownOutcome::Abandoned {
            jobs: 1,
            history_flushed: true
        }
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
        counts: ActiveJobCounts {
            total: 1,
            change: 0,
        },
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
        &ShutdownOutcome::Abandoned {
            jobs: 1,
            history_flushed: true
        }
    );
    Ok(())
}

#[test]
fn an_unconfirmed_leave_without_waiting_during_a_drain_with_change_work_changes_nothing()
-> Result<(), Box<dyn std::error::Error>> {
    let stuck = id(11)?;
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        active: HashSet::from([stuck]),
        counts: ActiveJobCounts {
            total: 1,
            change: 1,
        },
        ..FakePorts::default()
    };
    let timeouts = ShutdownTimeouts {
        os_return: Duration::ZERO,
        ..FAST
    };
    let mut coordinator = drain_ready_coordinator(ports, timeouts, QuitMode::Waiting)?;
    coordinator.advance(3)?;
    let events_before = coordinator.ports.events.len();

    assert_eq!(
        coordinator.request_quit(QuitMode::WithoutWaiting, false)?,
        QuitAdmission::ConfirmRequired {
            active_change_jobs: 1
        }
    );
    assert_eq!(coordinator.mode, QuitMode::Waiting, "the mode is unchanged");
    assert!(coordinator.os_return_deadline.is_none());
    assert_eq!(coordinator.ports.events.len(), events_before);
    coordinator.advance(4)?;
    assert_eq!(
        coordinator.phase,
        Phase::Draining,
        "the drain keeps waiting"
    );
    assert_eq!(coordinator.outcome(), &ShutdownOutcome::Clean);

    assert_eq!(
        coordinator.request_quit(QuitMode::WithoutWaiting, true)?,
        QuitAdmission::AlreadyClosing
    );
    assert_eq!(coordinator.mode, QuitMode::WithoutWaiting);
    coordinator.advance(5)?;
    assert_eq!(coordinator.phase, Phase::ZeroizingSecrets);
    Ok(())
}

#[test]
fn finished_job_history_is_flushed_before_the_engine_is_released()
-> Result<(), Box<dyn std::error::Error>> {
    let stuck = id(12)?;
    let ports = FakePorts {
        gateway_reports: VecDeque::from([closed_gateway(false)]),
        active: HashSet::from([stuck]),
        flush_statuses: VecDeque::from([HistoryCommitStatus::Pending]),
        ..FakePorts::default()
    };
    let timeouts = ShutdownTimeouts {
        os_return: Duration::ZERO,
        ..FAST
    };
    let mut coordinator = drain_ready_coordinator(ports, timeouts, QuitMode::WithoutWaiting)?;
    coordinator.advance(3)?;
    assert_eq!(
        coordinator.phase,
        Phase::Draining,
        "a flush still running holds the abandonment"
    );
    assert!(!coordinator.ports.released);
    coordinator.advance(4)?;
    assert_eq!(coordinator.phase, Phase::ZeroizingSecrets);
    assert_eq!(
        coordinator.outcome(),
        &ShutdownOutcome::Abandoned {
            jobs: 1,
            history_flushed: true
        }
    );
    let calls = &coordinator.ports.calls;
    let position = |name: &str| calls.iter().position(|call| *call == name);
    assert_eq!(
        calls.iter().filter(|call| **call == "flush_begin").count(),
        1
    );
    assert!(position("flush_poll") < position("release_engine"));
    Ok(())
}

#[test]
fn a_flush_that_cannot_complete_records_the_possible_history_loss()
-> Result<(), Box<dyn std::error::Error>> {
    for (statuses, start_fails) in [
        (VecDeque::from([HistoryCommitStatus::TimedOut]), false),
        (VecDeque::from([HistoryCommitStatus::Failed]), false),
        (VecDeque::new(), true),
    ] {
        let ports = FakePorts {
            gateway_reports: VecDeque::from([closed_gateway(false)]),
            active: HashSet::from([id(13)?]),
            flush_statuses: statuses,
            flush_start_fails: start_fails,
            ..FakePorts::default()
        };
        let timeouts = ShutdownTimeouts {
            drain: Duration::ZERO,
            ..FAST
        };
        let mut coordinator = drain_ready_coordinator(ports, timeouts, QuitMode::Waiting)?;
        coordinator.advance(3)?;
        assert_eq!(
            coordinator.outcome(),
            &ShutdownOutcome::DrainTimedOut {
                jobs: 1,
                history_flushed: false
            }
        );
        assert!(
            coordinator
                .diagnostics()
                .iter()
                .any(|note| note.contains("may be lost")),
            "the possible loss is logged"
        );
        assert!(coordinator.ports.released, "the abandonment still proceeds");
    }
    Ok(())
}

#[test]
fn the_confirmation_notice_counts_all_remaining_jobs_apart_from_change_jobs()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        counts: ActiveJobCounts {
            total: 3,
            change: 1,
        },
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::new(ports);
    coordinator.announce_running(1)?;
    coordinator.announce_confirmation_needed(1, 2)?;
    let events = coordinator.into_ports().events;
    let notice = events.last().ok_or("missing notice")?;
    assert_eq!(
        (notice.status, notice.previous),
        (LifecycleStatus::Running, LifecycleStatus::Running)
    );
    assert_eq!(notice.remaining_tasks, 3, "every remaining job");
    assert_eq!(notice.confirmation_needed, Some(1), "change jobs only");
    assert_eq!(events[0].confirmation_needed, None);
    assert!(events[0].sequence < notice.sequence);
    Ok(())
}

#[test]
fn a_forced_gateway_close_keeps_the_counts_of_the_failed_attempts()
-> Result<(), Box<dyn std::error::Error>> {
    let ports = FakePorts {
        gateway_failures_left: 3,
        force_gate: true,
        gateway_counts: (4, 5),
        ..FakePorts::default()
    };
    let mut coordinator = ShutdownCoordinator::with_timeouts(ports, FAST);
    coordinator.request_waiting_quit(true)?;
    assert!(coordinator.advance(1).is_err());
    assert!(coordinator.advance(2).is_err());
    coordinator.advance(3)?;
    let report = coordinator.gateway_report().ok_or("missing report")?;
    assert_eq!((report.sessions_closed, report.responses_discarded), (4, 5));
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
        &ShutdownOutcome::DrainTimedOut {
            jobs: 1,
            history_flushed: true
        }
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
