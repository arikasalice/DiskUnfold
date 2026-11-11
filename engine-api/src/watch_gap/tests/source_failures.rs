use super::*;
use crate::watch_source::deliver;
use du_core_contracts::change_notify::{ChangeHint, WatchError, WatchEvent};
use du_scan_engine::watch_lifecycle::{Action, Capability, Registration, Registry, State, Support};

fn ready() -> (Registry, u64, Ledger, Sink) {
    let mut registry = Registry::new(1, 1, Default::default()).unwrap();
    let caps = Capability {
        notification: Support::Supported,
        loss: Support::Supported,
        interruption: Support::Supported,
        path: Support::Unsupported,
        resource_limit: Support::Supported,
    };
    let def = definition(3);
    let start = registry.start(def.clone(), &[2], caps, 1).unwrap();
    let token = start
        .actions
        .iter()
        .find_map(|action| match action {
            Action::Register { token, .. } => Some(*token),
            _ => None,
        })
        .unwrap();
    registry
        .registration(
            [3; 16],
            token,
            1,
            &Registration::Acquired {
                handle: 7,
                uncovered: vec![vec![]],
            },
            2,
        )
        .unwrap();
    let mut consumer = Ledger::new([1; 16]);
    consumer.register::<()>(def).unwrap();
    let mut sink = Sink::default();
    snap(
        &mut consumer,
        &registry.incremental_snapshot([3; 16], 3).unwrap(),
        &mut sink,
    )
    .unwrap();
    (registry, token, consumer, sink)
}

#[test]
fn interruptions_flush_and_fence_with_unavailable_gap() {
    for reason in [
        WatchError::Disconnected,
        WatchError::AccessLost,
        WatchError::Other,
        WatchError::ResourceLimit,
    ] {
        let (mut registry, token, mut consumer, mut sink) = ready();
        let signal = WatchEvent::Signal {
            path: Some(vec![1]),
            hint: ChangeHint::Delete,
            observed_at: 4,
        };
        assert!(
            deliver(&mut registry, [3; 16], token, 1, &signal)
                .unwrap()
                .is_empty()
        );
        let interrupted = WatchEvent::Interrupted {
            reason,
            observed_at: 5,
        };
        let actions = deliver(&mut registry, [3; 16], token, 1, &interrupted).unwrap();
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::Status(status) if status.state == State::Lost))
        );
        assert!(actions.iter().any(|a| matches!(a, Action::Changes(batch) if batch.gaps.iter().any(|gap| gap.reason == GapReason::Unavailable && gap.end.is_none()))));
        for action in actions {
            if let Action::Changes(batch) = action {
                assert_eq!(
                    apply(&mut consumer, &batch, &mut sink),
                    Ok(Outcome::Applied)
                );
            }
        }
        assert!(
            consumer
                .unit(&[3; 16])
                .unwrap()
                .gaps
                .values()
                .any(|gap| gap.reason == GapReason::Unavailable && gap.end.is_none())
        );
        assert!(
            deliver(&mut registry, [3; 16], token, 1, &interrupted)
                .unwrap()
                .is_empty()
        );
        assert!(
            deliver(&mut registry, [3; 16], token, 1, &signal)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn failed_sink_does_not_advance_consumer_and_requires_snapshot() {
    let (mut registry, token, mut consumer, mut sink) = ready();
    let before = consumer.unit(&[3; 16]).unwrap().last_change_sequence;
    let loss = WatchEvent::Loss { observed_at: 4 };
    let actions = deliver(&mut registry, [3; 16], token, 1, &loss).unwrap();
    let batch = actions
        .into_iter()
        .find_map(|a| match a {
            Action::Changes(b) => Some(b),
            _ => None,
        })
        .unwrap();
    sink.fail = true;
    assert!(matches!(
        apply(&mut consumer, &batch, &mut sink),
        Err(Error::Apply("failed"))
    ));
    let unit = consumer.unit(&[3; 16]).unwrap();
    assert_eq!(unit.last_change_sequence, before);
    assert!(unit.needs_resync);
    sink.fail = false;
    assert_eq!(apply(&mut consumer, &batch, &mut sink), Ok(Outcome::Resync));
    let snapshot = registry.incremental_snapshot([3; 16], 5).unwrap();
    assert_eq!(
        snap(&mut consumer, &snapshot, &mut sink),
        Ok(Outcome::Applied)
    );
    assert!(
        consumer
            .unit(&[3; 16])
            .unwrap()
            .gaps
            .values()
            .any(|gap| gap.reason == GapReason::SignalLost)
    );
}

#[test]
fn rejected_regressing_time_preserves_registry_for_later_delivery() {
    let (mut registry, token, _, _) = ready();
    let before = registry.incremental_snapshot([3; 16], 3).unwrap();
    let stale_time = WatchEvent::Loss { observed_at: 1 };
    assert!(deliver(&mut registry, [3; 16], token, 1, &stale_time).is_err());
    assert_eq!(registry.incremental_snapshot([3; 16], 3).unwrap(), before);
    let timely = WatchEvent::Loss { observed_at: 4 };
    assert!(
        deliver(&mut registry, [3; 16], token, 1, &timely)
            .unwrap()
            .iter()
            .any(|a| matches!(a, Action::Changes(_)))
    );
}

#[test]
fn failed_registrations_never_accept_callbacks_or_claim_active() {
    for outcome in [
        Registration::Disconnected,
        Registration::AccessLost,
        Registration::Other,
        Registration::ResourceLimit,
    ] {
        let mut registry = Registry::new(1, 1, Default::default()).unwrap();
        let caps = Capability {
            notification: Support::Supported,
            loss: Support::Supported,
            interruption: Support::Supported,
            path: Support::Unsupported,
            resource_limit: Support::Supported,
        };
        let started = registry.start(definition(3), &[2], caps, 1).unwrap();
        let token = started
            .actions
            .iter()
            .find_map(|a| match a {
                Action::Register { token, .. } => Some(*token),
                _ => None,
            })
            .unwrap();
        registry
            .registration([3; 16], token, 1, &outcome, 2)
            .unwrap();
        let cut = registry.incremental_snapshot([3; 16], 3).unwrap();
        assert!(matches!(cut.state, State::Lost | State::Unsupported));
        if !matches!(outcome, Registration::ResourceLimit) {
            assert!(
                cut.gaps
                    .iter()
                    .any(|gap| gap.reason == GapReason::Unavailable && gap.end.is_none())
            );
        }
        let callback = WatchEvent::Signal {
            path: None,
            hint: ChangeHint::Unknown,
            observed_at: 4,
        };
        assert!(
            deliver(&mut registry, [3; 16], token, 1, &callback)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn stale_attempt_or_generation_cannot_mutate_active_watch() {
    let (mut registry, token, _, _) = ready();
    let before = registry.incremental_snapshot([3; 16], 3).unwrap();
    let loss = WatchEvent::Loss { observed_at: 4 };
    assert!(
        deliver(&mut registry, [3; 16], token + 1, 1, &loss)
            .unwrap()
            .is_empty()
    );
    assert!(
        deliver(&mut registry, [3; 16], token, 2, &loss)
            .unwrap()
            .is_empty()
    );
    assert_eq!(registry.incremental_snapshot([3; 16], 3).unwrap(), before);
}
