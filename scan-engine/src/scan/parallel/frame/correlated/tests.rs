use super::*;
use du_core_contracts::scan_observation::{
    AnchoredDirectory, DirectoryObservations, FilterCallbackPhase, HandleIdentity, ObservedEntry,
};
use std::collections::VecDeque;

struct Source {
    supported: bool,
    events: VecDeque<CorrelatedDirectoryEvent>,
}

fn unsupported() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "os.unsupported".into(),
        os_code: None,
    }
}

impl DirectoryObservations for Source {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }

    fn begin_correlated_filtered(&mut self) -> Result<(), PortError> {
        if self.supported {
            Ok(())
        } else {
            Err(unsupported())
        }
    }

    fn next_correlated_event(
        &mut self,
        _: &mut dyn FnMut(&[u8]) -> FilterDecision,
        _: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<CorrelatedDirectoryEvent, PortError> {
        self.events.pop_front().ok_or_else(unsupported)
    }
}

impl AnchoredDirectory for Source {
    fn observed_identity(&self) -> Option<HandleIdentity> {
        None
    }

    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(unsupported())
    }
}

fn admitted(child_id: u64, raw_name: Vec<u8>) -> CorrelatedDirectoryEvent {
    CorrelatedDirectoryEvent::FilterCallback {
        child_id,
        raw_name,
        decision: FilterDecision::Admit,
        phase: FilterCallbackPhase::BeforeMetadata,
    }
}

fn selected(source: Source) -> (Directory, Attempt) {
    let directory = Directory::opened(Box::new(source));
    let mut attempt = Attempt::default();
    let budget = CorrelatedBudget::new();
    assert!(matches!(
        attempt.select(&directory, &budget),
        Step::Progress
    ));
    (directory, attempt)
}

fn next(attempt: &mut Attempt, directory: &Directory) -> Step {
    attempt.next(directory, &mut |_| FilterDecision::Admit, &|| None, &|| {
        false
    })
}

#[test]
fn interleaved_admits_remain_bound_until_processed() {
    let (directory, mut attempt) = selected(Source {
        supported: true,
        events: VecDeque::from([
            admitted(0, b"alpha".to_vec()),
            admitted(1, b"beta".to_vec()),
            CorrelatedDirectoryEvent::End {
                child_count_exclusive: 2,
            },
        ]),
    });
    assert!(matches!(
        next(&mut attempt, &directory),
        Step::Event { seq: 0, .. }
    ));
    assert!(matches!(
        next(&mut attempt, &directory),
        Step::Event { seq: 1, .. }
    ));
    assert!(attempt.matches_callback(0, b"alpha"));
    assert!(attempt.matches_callback(1, b"beta"));
    assert!(!attempt.matches_callback(0, b"different"));
    attempt.finish_child(1);
    assert!(attempt.matches_callback(0, b"alpha"));
    assert!(!attempt.saw_callback(1));
    attempt.finish_child(0);
    assert!(matches!(
        next(&mut attempt, &directory),
        Step::End {
            event_watermark_exclusive: 2,
            child_count_exclusive: 2,
        }
    ));
}

#[test]
fn duplicate_or_pending_limit_keeps_attempt_unfinished() -> Result<(), String> {
    for ids in [vec![0, 0], (0..=MAX_PENDING_ADMITS as u64).collect()] {
        let events = ids
            .into_iter()
            .map(|id| admitted(id, id.to_le_bytes().to_vec()))
            .collect();
        let (directory, mut attempt) = selected(Source {
            supported: true,
            events,
        });
        let mut accepted = 0;
        loop {
            match next(&mut attempt, &directory) {
                Step::Event { .. } => accepted += 1,
                Step::Failed => break,
                _ => return Err("unexpected attempt state".into()),
            }
        }
        assert!(accepted <= MAX_PENDING_ADMITS);
        assert!(attempt.pending_admits.len() <= MAX_PENDING_ADMITS);
        attempt.cancel();
        assert_eq!(attempt.pending_admits.capacity(), 0);
        assert_eq!(attempt.pending_name_bytes, 0);
    }
    Ok(())
}

#[test]
fn callback_name_budget_exhaustion_does_not_finish_attempt() {
    let event = admitted(0, vec![b'x'; MAX_PENDING_NAME_BYTES + 1]);
    let (directory, mut attempt) = selected(Source {
        supported: true,
        events: VecDeque::from([event]),
    });
    assert!(matches!(next(&mut attempt, &directory), Step::Failed));
    assert_eq!(attempt.pending_name_bytes, 0);
}

#[test]
fn eof_with_unmatched_admit_fails() {
    let (directory, mut attempt) = selected(Source {
        supported: true,
        events: VecDeque::from([
            admitted(0, b"open".to_vec()),
            CorrelatedDirectoryEvent::End {
                child_count_exclusive: 1,
            },
        ]),
    });
    assert!(matches!(
        next(&mut attempt, &directory),
        Step::Event { seq: 0, .. }
    ));
    assert!(matches!(next(&mut attempt, &directory), Step::Failed));
}

#[test]
fn unsupported_only_falls_back_before_read() {
    let directory = Directory::opened(Box::new(Source {
        supported: false,
        events: VecDeque::new(),
    }));
    let mut attempt = Attempt::default();
    let budget = CorrelatedBudget::new();
    assert!(matches!(attempt.select(&directory, &budget), Step::Legacy));
    assert!(matches!(next(&mut attempt, &directory), Step::Failed));
}

#[test]
fn exclude_is_one_event_and_end_keeps_separate_count() {
    let (directory, mut attempt) = selected(Source {
        supported: true,
        events: VecDeque::from([
            CorrelatedDirectoryEvent::FilterCallback {
                child_id: 0,
                raw_name: b"excluded".to_vec(),
                decision: FilterDecision::Exclude,
                phase: FilterCallbackPhase::AfterMetadata,
            },
            CorrelatedDirectoryEvent::End {
                child_count_exclusive: 1,
            },
        ]),
    });
    assert!(matches!(
        next(&mut attempt, &directory),
        Step::Event {
            seq: 0,
            payload: CorrelatedDirectoryEvent::FilterCallback {
                raw_name,
                decision: FilterDecision::Exclude,
                phase: FilterCallbackPhase::AfterMetadata,
                ..
            },
            observation: None,
        } if raw_name == b"excluded"
    ));
    assert!(matches!(
        next(&mut attempt, &directory),
        Step::End {
            event_watermark_exclusive: 1,
            child_count_exclusive: 1,
        }
    ));
}

#[test]
fn cancellation_after_adapter_end_never_returns_eof() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (directory, mut attempt) = selected(Source {
        supported: true,
        events: VecDeque::from([CorrelatedDirectoryEvent::End {
            child_count_exclusive: 0,
        }]),
    });
    let checks = AtomicUsize::new(0);
    let cancelled = || checks.fetch_add(1, Ordering::SeqCst) != 0;
    assert!(matches!(
        attempt.next(
            &directory,
            &mut |_| FilterDecision::Admit,
            &|| None,
            &cancelled
        ),
        Step::Failed
    ));
}

#[test]
fn cancellation_during_read_returns_observation_to_caller_once() {
    use du_core_contracts::scan_observation::{ObservedKind, TimedObservedEntry};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let entry = ObservedEntry {
        name: b"observed".to_vec(),
        enumerated_inode: 0,
        enumerated_kind: ObservedKind::Unknown,
        metadata: Err(unsupported()),
    };
    let (directory, mut attempt) = selected(Source {
        supported: true,
        events: VecDeque::from([CorrelatedDirectoryEvent::Observation {
            child_id: 0,
            entry: TimedObservedEntry::from(entry),
        }]),
    });
    let checks = AtomicUsize::new(0);
    let cancelled = || checks.fetch_add(1, Ordering::SeqCst) != 0;
    assert!(matches!(
        attempt.next(
            &directory,
            &mut |_| FilterDecision::Admit,
            &|| None,
            &cancelled
        ),
        Step::Returned(CorrelatedDirectoryEvent::Observation { child_id: 0, .. })
    ));
    assert!(matches!(next(&mut attempt, &directory), Step::Failed));
}

#[test]
fn transported_observation_keeps_child_id_for_worker() {
    use du_core_contracts::scan_observation::{ObservedKind, TimedObservedEntry};
    let entry = ObservedEntry {
        name: b"observed".to_vec(),
        enumerated_inode: 0,
        enumerated_kind: ObservedKind::Unknown,
        metadata: Err(unsupported()),
    };
    let (directory, mut attempt) = selected(Source {
        supported: true,
        events: VecDeque::from([CorrelatedDirectoryEvent::Observation {
            child_id: 7,
            entry: TimedObservedEntry::from(entry),
        }]),
    });
    assert!(matches!(
        next(&mut attempt, &directory),
        Step::Event {
            seq: 0,
            payload: CorrelatedDirectoryEvent::Observation { child_id: 7, .. },
            observation: Some(QueuedEntry {
                child_id: Some(7),
                ..
            }),
        }
    ));
}
