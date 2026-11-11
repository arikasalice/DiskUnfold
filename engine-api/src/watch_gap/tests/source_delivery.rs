use super::*;

#[test]
fn port_events_reach_registry_batches_and_the_consumer() {
    use crate::watch_source::deliver;
    use du_core_contracts::change_notify::{ChangeHint, WatchEvent};
    use du_scan_engine::watch_lifecycle::{Action, Capability, Registration, Registry, Support};
    let mut producer = Registry::new(1, 1, Default::default()).unwrap();
    let caps = Capability {
        notification: Support::Supported,
        loss: Support::Supported,
        interruption: Support::Supported,
        path: Support::Unsupported,
        resource_limit: Support::Supported,
    };
    let definition = definition(3);
    let start = producer.start(definition.clone(), &[2], caps, 1).unwrap();
    let token = start
        .actions
        .iter()
        .find_map(|action| match action {
            Action::Register { token, .. } => Some(*token),
            _ => None,
        })
        .unwrap();
    producer
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
    consumer.register::<()>(definition).unwrap();
    let mut sink = Sink::default();
    snap(
        &mut consumer,
        &producer.incremental_snapshot([3; 16], 3).unwrap(),
        &mut sink,
    )
    .unwrap();
    let change = WatchEvent::Signal {
        path: None,
        hint: ChangeHint::Unknown,
        observed_at: 4,
    };
    assert!(
        deliver(&mut producer, [3; 16], token, 1, &change)
            .unwrap()
            .is_empty()
    );
    let actions = producer.tick([3; 16], token, 1, 1004).unwrap();
    for action in actions {
        if let Action::Changes(batch) = action {
            assert!(batch.changes.iter().any(|change| change.path.is_empty()));
            assert_eq!(
                apply(&mut consumer, &batch, &mut sink),
                Ok(Outcome::Applied)
            );
        }
    }
    let loss = WatchEvent::Loss { observed_at: 1005 };
    let actions = deliver(&mut producer, [3; 16], token, 1, &loss).unwrap();
    assert!(actions.iter().any(|action| matches!(action, Action::Changes(batch) if batch.gaps.iter().any(|gap| gap.reason == GapReason::SignalLost))));
    for action in actions {
        if let Action::Changes(batch) = action {
            assert_eq!(
                apply(&mut consumer, &batch, &mut sink),
                Ok(Outcome::Applied)
            );
        }
    }
    let lost = consumer
        .unit(&[3; 16])
        .unwrap()
        .gaps
        .values()
        .find(|gap| gap.reason == GapReason::SignalLost)
        .unwrap();
    let classified = recovery::classify_gap(lost);
    assert_eq!(classified.scope, recovery::GapScope::Unit);
    assert_eq!(
        classified.range_evidence.map(|evidence| {
            recovery::decide(true, recovery::BaseResult::Compatible, true, evidence)
        }),
        Some(recovery::Decision::RecoverTarget(
            recovery::RecoveryReason::GapUnlocatable
        ))
    );
    producer.stop([3; 16], 1006).unwrap();
    assert!(
        deliver(&mut producer, [3; 16], token, 1, &change)
            .unwrap()
            .is_empty()
    );
}
