mod fixtures;

use du_change_planner::{
    api::PlanBuilder,
    ports::{
        ConfirmationChoice, ConfirmationToken, EstimateQuality, EvaluatorId, PlanAction,
        PlanItemState, ProtectionVerdict, RestoreRestriction,
    },
    readiness::PlanReadiness,
};
use du_core_contracts::{OsPath, PathBytes, PathEncoding, Timestamp, volume::CapabilityState};
use fixtures::{FakePorts, id, probe_registry_with, request, selection, size_value};

#[test]
fn fr8_hd1_app_host_dd1_counts_each_file_identity_once() {
    let fake = FakePorts::standard();
    fake.set_item(
        1,
        fixtures::item_facts(b"/root/a", &[b"root", b"a"], b"shared-file", Some(12)),
    );
    fake.set_item(
        2,
        fixtures::item_facts(b"/root/z", &[b"root", b"z"], b"shared-file", Some(12)),
    );
    let plan = fake.planner().build(request(
        &fake,
        vec![selection(&fake, 1), selection(&fake, 2)],
    ));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        assert_eq!(plan.expected_effect.len(), 1);
        assert_eq!(plan.expected_effect[0].known_bytes, 12);
        assert_eq!(plan.expected_effect[0].unconfirmed_items, 0);
    }
}

#[test]
fn fr8_hd1_app_host_dd1_keeps_unknown_size_as_unconfirmed() {
    let mut fake = FakePorts::standard();
    fake.default_item.size = None;
    let plan = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        assert_eq!(plan.expected_effect[0].known_bytes, 0);
        assert_eq!(plan.expected_effect[0].unconfirmed_items, 1);
        assert_eq!(
            plan.expected_effect[0].quality,
            EstimateQuality::Unconfirmed
        );
    }
}

#[test]
fn fr9_hd1_app_host_dd1_maps_restore_capabilities_for_each_action() {
    let cases = [
        (
            PlanAction::Trash,
            Some(CapabilityState::Supported),
            RestoreRestriction::Supported,
        ),
        (
            PlanAction::Trash,
            Some(CapabilityState::Partial),
            RestoreRestriction::Supported,
        ),
        (
            PlanAction::Trash,
            Some(CapabilityState::Unsupported),
            RestoreRestriction::Unsupported,
        ),
        (PlanAction::Trash, None, RestoreRestriction::Unconfirmed),
        (PlanAction::Move, None, RestoreRestriction::Supported),
        (PlanAction::Delete, None, RestoreRestriction::Unsupported),
    ];
    for (action, capability, expected) in cases {
        let mut fake = FakePorts::standard();
        fake.platform.trash_capability = capability;
        let mut chosen = selection(&fake, 1);
        chosen.action = action;
        if action == PlanAction::Move {
            chosen.destination = Some(path(b"/root/moved"));
        }
        let plan = fake.planner().build(request(&fake, vec![chosen]));
        assert!(plan.is_ok(), "{action:?}");
        if let Ok(plan) = plan {
            assert_eq!(plan.items[0].restore, expected, "{action:?}");
        }
    }
}

#[test]
fn fr10_hd1_app_host_dd1_prepares_confirmation_tokens_for_released_and_overwritten_items() {
    let fake = FakePorts::standard();
    let registry = probe_registry_with(
        None,
        &[(EvaluatorId::GuardAge, ProtectionVerdict::AllowReleased)],
    );
    let builder = fake.planner_with(registry);
    assert!(builder.is_ok());
    let Ok(builder) = builder else {
        return;
    };
    let mut release_request = request(&fake, vec![selection(&fake, 1)]);
    release_request
        .confirmations
        .push(ConfirmationChoice::ReleaseDefaultProtection);
    let released = builder.build(release_request);
    assert!(released.is_ok());
    if let Ok(released) = released {
        assert!(released.confirmations.iter().any(|token| matches!(
            token,
            ConfirmationToken::ReleaseDefaultProtection { classification_ids }
                if classification_ids == &["user-file".to_owned()]
        )));
    }

    let mut overwrite = selection(&fake, 2);
    overwrite.action = PlanAction::Move;
    overwrite.destination = Some(path(b"/root/destination"));
    overwrite.overwrite = true;
    overwrite.displaced_size = Some(size_value(7));
    let mut overwrite_request = request(&fake, vec![overwrite]);
    overwrite_request
        .confirmations
        .push(ConfirmationChoice::Overwrite);
    let overwritten = builder.build(overwrite_request);
    assert!(overwritten.is_ok());
    if let Ok(overwritten) = overwritten {
        assert_eq!(overwritten.items[0].state, PlanItemState::Included);
        assert_eq!(overwritten.expected_effect[0].known_bytes, 7);
        assert!(overwritten.confirmations.iter().any(|token| matches!(
            token,
            ConfirmationToken::Overwrite {
                item_count: 1,
                displaced_bytes: 7
            }
        )));
    }
}

#[test]
fn fr10_hd1_app_host_dd1_readiness_uses_sample_confirmation_target_rows() {
    let mut sample = FakePorts::standard();
    sample.source = du_core_contracts::data_source::DataSource::Sample;
    let sample_plan = sample
        .planner()
        .build(request(&sample, vec![selection(&sample, 1)]));
    assert!(sample_plan.is_ok());
    if let Ok(plan) = sample_plan {
        assert_eq!(plan.readiness, PlanReadiness::SamplePlan);
    }

    let mut held = FakePorts::standard();
    held.target_check = du_change_planner::ports::TargetCheck::Changed;
    let held_plan = held
        .planner()
        .build(request(&held, vec![selection(&held, 1)]));
    assert!(held_plan.is_ok());
    if let Ok(plan) = held_plan {
        assert_eq!(plan.readiness, PlanReadiness::NeedsConfirmation);
    }

    let mut empty_target = FakePorts::standard();
    empty_target.platform.identity = du_change_planner::ports::Fact::Known(
        du_change_planner::ports::IdentityObservation::Mismatch,
    );
    let no_target = empty_target
        .planner()
        .build(request(&empty_target, vec![selection(&empty_target, 1)]));
    assert!(no_target.is_ok());
    if let Ok(plan) = no_target {
        assert_eq!(plan.readiness, PlanReadiness::NoTarget);
    }
}

#[test]
fn fr10_hd1_app_host_dd1_hash_is_stable_and_obeys_the_v1_scope() {
    let fake = FakePorts::standard();
    let first = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    let second = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    assert!(first.is_ok() && second.is_ok());
    let (Ok(first), Ok(second)) = (first, second) else {
        return;
    };
    assert_eq!(first.hash, second.hash);

    let mut excluded = first.clone();
    excluded.version_id = id(100);
    excluded.request_channel = du_change_planner::ports::RequestChannel::Cli;
    excluded.created_at =
        Timestamp::from_unix_millis(1_800_000_000_000).unwrap_or_else(|| first.created_at.clone());
    assert_eq!(
        PlanBuilder::hash_version(&excluded),
        first.hash,
        "version ID, creation time, and channel are outside the hash scope"
    );

    let mut included = first.clone();
    included.expected_effect[0].known_bytes += 1;
    assert_ne!(PlanBuilder::hash_version(&included), first.hash);
    included = first.clone();
    included.source_version_id = id(101);
    assert_ne!(PlanBuilder::hash_version(&included), first.hash);
}

#[test]
fn fr10_hd1_app_host_dd1_requires_overwrite_confirmation() {
    let fake = FakePorts::standard();
    let mut overwrite = selection(&fake, 1);
    overwrite.action = PlanAction::Move;
    overwrite.destination = Some(path(b"/root/destination"));
    overwrite.overwrite = true;
    overwrite.displaced_size = Some(size_value(7));
    let plan = fake.planner().build(request(&fake, vec![overwrite]));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        assert_eq!(plan.items[0].state, PlanItemState::Held);
        assert_eq!(
            plan.items[0].reason_key.as_deref(),
            Some("plan.item.needs_confirmation")
        );
        assert!(plan.confirmations.is_empty());
    }
}

fn path(raw: &[u8]) -> OsPath {
    OsPath {
        encoding: PathEncoding::Unix,
        raw: PathBytes::from_bytes(raw),
        display: String::from_utf8_lossy(raw).into_owned(),
        lossy: false,
    }
}
