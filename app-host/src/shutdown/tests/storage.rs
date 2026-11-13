use super::*;

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
