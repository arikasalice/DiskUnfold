mod fixtures;

use du_change_planner::{
    api::{EvaluatorRegistry, EvaluatorRegistryError},
    ports::{
        ConfirmationChoice, ConfirmationToken, EvaluatorId, Fact, IdentityObservation,
        KnownLocation, KnownLocationKind, PlanItemState, ProtectionVerdict,
    },
};
use fixtures::{
    FakePorts, item_facts, platform_facts, probe_registry, probe_registry_with, request, selection,
};

#[test]
fn fr7_hd1_app_host_dd1_maps_evaluator_errors_to_undetermined_with_original_evidence() {
    let fake = FakePorts::standard();
    let builder = fake.planner_with(probe_registry(Some(EvaluatorId::GuardAge)));
    assert!(builder.is_ok());
    let Ok(builder) = builder else {
        return;
    };
    let plan = builder.build(request(&fake, vec![selection(&fake, 1)]));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        let item = &plan.items[0];
        assert_eq!(item.state, PlanItemState::Rejected);
        assert_eq!(item.reason_key.as_deref(), Some("plan.item.undetermined"));
        assert!(item.protection.is_some());
        let Some(summary) = &item.protection else {
            return;
        };
        assert_eq!(summary.verdict, ProtectionVerdict::Undetermined);
        assert!(
            summary
                .evidence
                .iter()
                .any(|key| key == "protect.injected_failure")
        );
    }
}

#[test]
fn fr7_hd1_app_host_dd1_makes_every_item_undetermined_when_ruleset_read_fails() {
    let mut fake = FakePorts::standard();
    fake.set_item(
        2,
        item_facts(b"/root/other", &[b"root", b"other"], b"other", Some(8)),
    );
    fake.ruleset = Err(fixtures::port_error("protect.ruleset_unreadable"));
    let plan = fake.planner().build(request(
        &fake,
        vec![selection(&fake, 1), selection(&fake, 2)],
    ));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        assert!(plan.items.iter().all(|item| {
            item.protection
                .as_ref()
                .is_some_and(|summary| summary.verdict == ProtectionVerdict::Undetermined)
        }));
        assert!(plan.items.iter().all(|item| {
            item.state == PlanItemState::Rejected
                && item.reason_key.as_deref() == Some("plan.item.undetermined")
        }));
    }
}

#[test]
fn fr7_hd1_app_host_dd1_denies_identity_mismatch_and_unavailable_identity() {
    for identity in [
        Fact::Known(IdentityObservation::Mismatch),
        Fact::Unknown(du_core_contracts::protection_facts::fact_reason::PARTIAL),
    ] {
        let mut fake = FakePorts::standard();
        fake.platform.identity = identity;
        let plan = fake
            .planner()
            .build(request(&fake, vec![selection(&fake, 1)]));
        assert!(plan.is_ok());
        if let Ok(plan) = plan {
            let item = &plan.items[0];
            assert_eq!(item.state, PlanItemState::Rejected);
            let mismatch = matches!(identity, Fact::Known(IdentityObservation::Mismatch));
            let expected = if mismatch {
                "plan.item.protected"
            } else {
                "plan.item.undetermined"
            };
            assert_eq!(item.reason_key.as_deref(), Some(expected));
            assert!(item.protection.as_ref().is_some_and(|summary| {
                summary.verdict
                    == if mismatch {
                        ProtectionVerdict::Deny
                    } else {
                        ProtectionVerdict::Undetermined
                    }
                    && summary.evidence.iter().any(|key| {
                        key == if mismatch {
                            "protect.identity_unavailable"
                        } else {
                            du_core_contracts::protection_facts::fact_reason::PARTIAL
                        }
                    })
            }));
        }
    }
}

#[test]
fn fr7_hd1_app_host_dd1_skips_rows_five_to_seven_when_location_is_not_listed() {
    let fake = FakePorts::standard();
    let plan = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        assert_eq!(plan.items[0].state, PlanItemState::Included);
        assert!(
            plan.items[0]
                .protection
                .as_ref()
                .is_some_and(|summary| { summary.verdict == ProtectionVerdict::Allow })
        );
        assert_eq!(
            EvaluatorId::ORDERED,
            [
                EvaluatorId::Identity,
                EvaluatorId::LinkBoundary,
                EvaluatorId::VolumeRole,
                EvaluatorId::KnownLocation,
                EvaluatorId::OsComponent,
                EvaluatorId::ClassAction,
                EvaluatorId::ReleaseState,
                EvaluatorId::GuardAge,
                EvaluatorId::GuardPermission,
                EvaluatorId::GuardDiskBacked,
                EvaluatorId::UserRule,
                EvaluatorId::Descendant,
            ]
        );
        let executed = plan.items[0]
            .protection
            .as_ref()
            .map(|summary| {
                summary
                    .evaluators
                    .iter()
                    .map(|entry| entry.0)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert_eq!(
            executed,
            vec![
                EvaluatorId::Identity,
                EvaluatorId::LinkBoundary,
                EvaluatorId::VolumeRole,
                EvaluatorId::KnownLocation,
                EvaluatorId::GuardAge,
                EvaluatorId::GuardPermission,
                EvaluatorId::GuardDiskBacked,
                EvaluatorId::UserRule,
                EvaluatorId::Descendant,
            ]
        );
    }
}

#[test]
fn fr7_hd1_app_host_dd1_denies_a_known_location_root() {
    let mut fake = FakePorts::standard();
    fake.platform.known_location = Fact::Known(KnownLocation::Root);
    let plan = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        let item = &plan.items[0];
        assert_eq!(item.state, PlanItemState::Rejected);
        assert_eq!(item.reason_key.as_deref(), Some("plan.item.protected"));
        let Some(summary) = &item.protection else {
            return;
        };
        assert_eq!(summary.verdict, ProtectionVerdict::Deny);
        assert!(
            summary
                .evidence
                .iter()
                .any(|key| key == "protect.no_change")
        );
        assert_eq!(
            summary
                .evaluators
                .iter()
                .map(|entry| entry.0)
                .collect::<Vec<_>>(),
            vec![
                EvaluatorId::Identity,
                EvaluatorId::LinkBoundary,
                EvaluatorId::VolumeRole,
                EvaluatorId::KnownLocation,
            ]
        );
    }
}

#[test]
fn fr7_hd1_app_host_dd1_v1_allows_only_a_class_with_an_allow_action() {
    let mut fake = FakePorts::standard();
    fake.platform.known_location = Fact::Known(KnownLocation::Under(KnownLocationKind::UserCache));
    let mut plan_request = request(&fake, vec![selection(&fake, 1)]);
    plan_request
        .confirmations
        .push(ConfirmationChoice::ReleaseDefaultProtection);
    let plan = fake.planner().build(plan_request);
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        let item = &plan.items[0];
        assert_eq!(item.state, PlanItemState::Included);
        let Some(summary) = &item.protection else {
            return;
        };
        assert_eq!(summary.verdict, ProtectionVerdict::Allow);
        assert!(!summary.released);
        assert_eq!(
            summary
                .evaluators
                .iter()
                .map(|entry| entry.0)
                .collect::<Vec<_>>(),
            EvaluatorId::ORDERED.to_vec()
        );
        assert!(
            !plan
                .confirmations
                .iter()
                .any(|token| matches!(token, ConfirmationToken::ReleaseDefaultProtection { .. }))
        );
    }
}

#[test]
fn fr7_hd1_app_host_dd1_keeps_unknown_known_location_undetermined() {
    let mut fake = FakePorts::standard();
    fake.platform.known_location =
        Fact::Unknown(du_core_contracts::protection_facts::fact_reason::PARTIAL);
    let plan = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        let item = &plan.items[0];
        assert_eq!(item.state, PlanItemState::Rejected);
        assert_eq!(item.reason_key.as_deref(), Some("plan.item.undetermined"));
        assert!(item.protection.as_ref().is_some_and(|summary| {
            summary.verdict == ProtectionVerdict::Undetermined
                && summary
                    .evidence
                    .iter()
                    .any(|key| key == du_core_contracts::protection_facts::fact_reason::PARTIAL)
                && summary
                    .evaluators
                    .iter()
                    .map(|entry| entry.0)
                    .collect::<Vec<_>>()
                    == EvaluatorId::ORDERED.to_vec()
        }));
    }
}

#[test]
fn f29_missing_derived_classification_does_not_reuse_analysis_classification() {
    let mut fake = FakePorts::standard();
    fake.platform.classification_id = None;
    fake.platform.known_location = Fact::Known(KnownLocation::Under(KnownLocationKind::UserHome));
    let plan = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        let item = &plan.items[0];
        assert_eq!(item.state, PlanItemState::Rejected);
        assert_eq!(item.reason_key.as_deref(), Some("plan.item.undetermined"));
        assert!(
            item.protection
                .as_ref()
                .is_some_and(|summary| summary.classification_id.is_none())
        );
    }
}

#[test]
fn fr7_hd1_app_host_dd1_combines_verdicts_by_the_strictest_rank() {
    let mut fake = FakePorts::standard();
    fake.platform = platform_facts();
    let registry = probe_registry_with(
        None,
        &[
            (EvaluatorId::Identity, ProtectionVerdict::AllowReleased),
            (EvaluatorId::LinkBoundary, ProtectionVerdict::GuidanceOnly),
            (EvaluatorId::VolumeRole, ProtectionVerdict::Undetermined),
        ],
    );
    let builder = fake.planner_with(registry);
    assert!(builder.is_ok());
    let Ok(builder) = builder else {
        return;
    };
    let plan = builder.build(request(&fake, vec![selection(&fake, 1)]));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        assert_eq!(
            plan.items[0]
                .protection
                .as_ref()
                .map(|summary| summary.verdict),
            Some(ProtectionVerdict::Undetermined)
        );
    }
}

#[test]
fn fr7_hd1_app_host_dd1_rejects_duplicate_evaluator_ids_during_assembly() {
    let mut registry = EvaluatorRegistry::empty();
    let first = fixtures::ProbeEvaluator {
        id: EvaluatorId::Identity,
        decision: ProtectionVerdict::Allow,
        failure: None,
    };
    let duplicate = fixtures::ProbeEvaluator {
        id: EvaluatorId::Identity,
        decision: ProtectionVerdict::Allow,
        failure: None,
    };
    assert!(registry.register(Box::new(first)).is_ok());
    assert_eq!(
        registry.register(Box::new(duplicate)),
        Err(EvaluatorRegistryError::Duplicate(EvaluatorId::Identity))
    );
}
