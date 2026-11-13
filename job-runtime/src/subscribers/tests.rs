use super::*;
use crate::JobEvent;
use du_core_contracts::job::JobState;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Sample {
    job: Id,
    seq: u64,
    kind: Option<u8>,
    end: bool,
}
impl SubscriberEvent for Sample {
    type Kind = u8;
    fn job_sequence(&self) -> Option<(&Id, u64)> {
        Some((&self.job, self.seq))
    }
    fn current_kind(&self) -> Option<u8> {
        self.kind
    }
    fn is_terminal(&self) -> bool {
        self.end
    }
}
fn id(n: u8) -> Id {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = n;
    Id::from_v7_bytes(bytes)
}
fn sample(job: u8, seq: u64, kind: Option<u8>, end: bool) -> Sample {
    Sample {
        job: id(job),
        seq,
        kind,
        end,
    }
}
fn recv<E: SubscriberEvent>(s: &Subscription<E>, millis: u64) -> Option<SubscriberMessage<E>> {
    s.try_next(Duration::from_millis(millis))
}

#[test]
fn current_values_are_coalesced_by_job_and_kind_within_the_window() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let sub = bus.subscribe().ok_or("subscribe")?;
    for e in [
        sample(1, 1, Some(1), false),
        sample(1, 2, Some(2), false),
        sample(2, 1, Some(1), false),
        sample(1, 3, Some(1), false),
    ] {
        assert!(bus.publish(e, Duration::ZERO));
    }
    assert_eq!(recv(&sub, 99), None);
    assert_eq!(
        recv(&sub, 100),
        Some(SubscriberMessage::Event(sample(1, 2, Some(2), false)))
    );
    assert_eq!(
        recv(&sub, 100),
        Some(SubscriberMessage::Event(sample(2, 1, Some(1), false)))
    );
    assert_eq!(
        recv(&sub, 100),
        Some(SubscriberMessage::Event(sample(1, 3, Some(1), false)))
    );
    assert_eq!(recv(&sub, 200), None);
    Ok(())
}

#[test]
fn only_slow_subscriber_detaches_at_capacity_and_waits_for_snapshot_cut() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let slow = bus.subscribe().ok_or("slow")?;
    let fast = bus.subscribe().ok_or("fast")?;
    for seq in 1..=1023 {
        assert!(bus.publish(sample(1, seq, None, false), Duration::ZERO));
        assert!(recv(&fast, 100).is_some());
    }
    {
        let state = slow.state.lock().map_err(|_| "gate poisoned")?;
        let slot = state.slots.get(&slow.id).ok_or("slow slot")?;
        assert_eq!(slot.queue.len(), 1023);
        assert!(!slot.detached);
    }
    assert_eq!(recv(&slow, 99), None);
    assert!(bus.publish(sample(1, 1024, None, false), Duration::from_millis(100)));
    assert_eq!(
        recv(&slow, 100),
        Some(SubscriberMessage::ResyncRequired { dropped: 1024 })
    );
    assert!(bus.publish(sample(1, 1025, None, false), Duration::from_millis(100)));
    assert_eq!(recv(&slow, 200), None);
    assert!(recv(&fast, 200).is_some());
    let cut = HashMap::from([(
        id(1),
        SnapshotCut {
            sequence: 1025,
            terminal: false,
        },
    )]);
    let token = slow.begin_rejoin().ok_or("rejoin token")?;
    assert!(slow.complete_rejoin(token, cut));
    assert!(bus.publish(sample(1, 1025, None, false), Duration::from_millis(200)));
    assert!(bus.publish(sample(1, 1026, None, false), Duration::from_millis(200)));
    assert_eq!(
        recv(&slow, 300),
        Some(SubscriberMessage::Event(sample(1, 1026, None, false)))
    );
    Ok(())
}

#[test]
fn replacement_is_applied_before_the_capacity_check() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let sub = bus.subscribe().ok_or("subscribe")?;
    for seq in 1..=1022 {
        assert!(bus.publish(sample(1, seq, None, false), Duration::ZERO));
    }
    assert!(bus.publish(sample(1, 1023, Some(1), false), Duration::ZERO));
    assert!(bus.publish(sample(1, 1024, Some(1), false), Duration::ZERO));
    let state = sub.state.lock().map_err(|_| "gate poisoned")?;
    let slot = state.slots.get(&sub.id).ok_or("slot")?;
    assert_eq!(slot.queue.len(), 1023);
    assert!(!slot.detached);
    assert_eq!(slot.queue.back(), Some(&sample(1, 1024, Some(1), false)));
    Ok(())
}

#[test]
fn snapshot_overlapping_publication_requires_a_fresh_cut() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let sub = bus.subscribe().ok_or("subscribe")?;
    for seq in 1..=1024 {
        assert!(bus.publish(sample(1, seq, None, false), Duration::ZERO));
    }
    assert_eq!(
        recv(&sub, 100),
        Some(SubscriberMessage::ResyncRequired { dropped: 1024 })
    );
    let token = sub.begin_rejoin().ok_or("token")?;
    assert!(bus.publish(sample(1, 1025, None, false), Duration::from_millis(100)));
    let old = HashMap::from([(
        id(1),
        SnapshotCut {
            sequence: 1024,
            terminal: false,
        },
    )]);
    assert!(!sub.complete_rejoin(token, old));
    let token = sub.begin_rejoin().ok_or("fresh token")?;
    let fresh = HashMap::from([(
        id(1),
        SnapshotCut {
            sequence: 1025,
            terminal: false,
        },
    )]);
    assert!(sub.complete_rejoin(token, fresh));
    assert!(bus.publish(sample(1, 1026, None, false), Duration::from_millis(100)));
    assert_eq!(
        recv(&sub, 200),
        Some(SubscriberMessage::Event(sample(1, 1026, None, false)))
    );
    Ok(())
}

#[test]
fn rejoin_token_cannot_cross_bus_instances() -> Result<(), String> {
    let first = SubscriberBus::default();
    let second = SubscriberBus::default();
    let one = first.subscribe().ok_or("first")?;
    let two = second.subscribe().ok_or("second")?;
    for seq in 1..=1024 {
        assert!(first.publish(sample(1, seq, None, false), Duration::ZERO));
        assert!(second.publish(sample(1, seq, None, false), Duration::ZERO));
    }
    assert_eq!(one.id, two.id);
    assert_eq!(
        first.0.lock().map_err(|_| "first gate")?.generation,
        second.0.lock().map_err(|_| "second gate")?.generation
    );
    let token = one.begin_rejoin().ok_or("first token")?;
    let cut = HashMap::from([(
        id(1),
        SnapshotCut {
            sequence: 1024,
            terminal: false,
        },
    )]);
    assert!(!two.complete_rejoin(token, cut.clone()));
    assert_eq!(
        recv(&two, 100),
        Some(SubscriberMessage::ResyncRequired { dropped: 1024 })
    );
    let own = two.begin_rejoin().ok_or("second token")?;
    assert!(two.complete_rejoin(own, cut));
    Ok(())
}

#[test]
fn current_values_across_deadline_are_not_coalesced() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let sub = bus.subscribe().ok_or("subscribe")?;
    assert!(bus.publish(sample(1, 1, Some(1), false), Duration::ZERO));
    assert!(bus.publish(sample(1, 2, Some(1), false), Duration::from_millis(100)));
    assert_eq!(
        recv(&sub, 100),
        Some(SubscriberMessage::Event(sample(1, 1, Some(1), false)))
    );
    assert_eq!(recv(&sub, 100), None);
    assert_eq!(
        recv(&sub, 200),
        Some(SubscriberMessage::Event(sample(1, 2, Some(1), false)))
    );
    Ok(())
}

#[test]
fn terminal_is_last_and_snapshot_terminal_rejects_late_events() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let sub = bus.subscribe().ok_or("subscribe")?;
    assert!(bus.publish(sample(1, 1, Some(1), false), Duration::ZERO));
    assert!(bus.publish(sample(1, 2, None, true), Duration::ZERO));
    assert!(bus.publish(sample(1, 3, Some(1), false), Duration::ZERO));
    assert_eq!(
        recv(&sub, 100),
        Some(SubscriberMessage::Event(sample(1, 1, Some(1), false)))
    );
    assert_eq!(
        recv(&sub, 100),
        Some(SubscriberMessage::Event(sample(1, 2, None, true)))
    );
    assert_eq!(recv(&sub, 100), None);
    Ok(())
}

#[test]
fn terminal_in_snapshot_blocks_reports_after_rejoin() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let sub = bus.subscribe().ok_or("subscribe")?;
    for seq in 1..=1024 {
        assert!(bus.publish(sample(1, seq, None, false), Duration::ZERO));
    }
    assert_eq!(
        recv(&sub, 100),
        Some(SubscriberMessage::ResyncRequired { dropped: 1024 })
    );
    let token = sub.begin_rejoin().ok_or("token")?;
    let cut = HashMap::from([(
        id(1),
        SnapshotCut {
            sequence: 1024,
            terminal: true,
        },
    )]);
    assert!(sub.complete_rejoin(token, cut));
    assert!(bus.publish(sample(1, 1025, Some(1), false), Duration::from_millis(100)));
    assert_eq!(recv(&sub, 200), None);
    Ok(())
}

#[test]
fn real_job_item_results_are_ordered_and_never_coalesced() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let sub = bus.subscribe().ok_or("subscribe")?;
    let event = |seq| JobEvent::ItemResult {
        job_id: id(1),
        kind_id: "scan.metadata".into(),
        sequence: seq,
        payload: vec![seq as u8],
    };
    assert!(bus.publish(event(1), Duration::ZERO));
    assert!(bus.publish(event(2), Duration::ZERO));
    assert_eq!(recv(&sub, 100), Some(SubscriberMessage::Event(event(1))));
    assert_eq!(recv(&sub, 100), Some(SubscriberMessage::Event(event(2))));
    let state = JobEvent::StateChanged {
        job_id: id(1),
        state: JobState::Running,
        sequence: 3,
    };
    assert!(bus.publish(state.clone(), Duration::from_millis(100)));
    assert_eq!(recv(&sub, 200), Some(SubscriberMessage::Event(state)));
    Ok(())
}

#[test]
fn concurrent_fast_and_stalled_subscribers_are_isolated() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let slow = bus.subscribe().ok_or("slow")?;
    let fast = bus.subscribe().ok_or("fast")?;
    let (published_tx, published_rx) = std::sync::mpsc::sync_channel(0);
    let (drained_tx, drained_rx) = std::sync::mpsc::sync_channel(0);
    let fast_thread = std::thread::spawn(move || -> Result<usize, String> {
        let mut count = 0;
        while let Ok(seq) = published_rx.recv() {
            let event = recv(&fast, 100).ok_or("fast event missing")?;
            if event != SubscriberMessage::Event(sample(1, seq, None, false)) {
                return Err("fast subscriber lost or reordered an event".into());
            }
            count += 1;
            drained_tx.send(()).map_err(|e| e.to_string())?;
        }
        Ok(count)
    });
    for seq in 1..=1024 {
        assert!(bus.publish(sample(1, seq, None, false), Duration::ZERO));
        published_tx.send(seq).map_err(|e| e.to_string())?;
        drained_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
    }
    drop(published_tx);
    assert_eq!(
        fast_thread.join().map_err(|_| "fast thread panicked")??,
        1024
    );
    assert_eq!(
        recv(&slow, 100),
        Some(SubscriberMessage::ResyncRequired { dropped: 1024 })
    );
    Ok(())
}

#[test]
fn retired_terminal_is_not_forgotten_while_queued() -> Result<(), String> {
    let bus = SubscriberBus::default();
    let sub = bus.subscribe().ok_or("subscribe")?;
    assert!(bus.publish(sample(1, 1, None, true), Duration::ZERO));
    assert!(!sub.forget_retired_job(&id(1)));
    assert!(recv(&sub, 100).is_some());
    assert!(sub.forget_retired_job(&id(1)));
    Ok(())
}
