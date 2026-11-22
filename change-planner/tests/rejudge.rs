mod fixtures;

use du_change_planner::{
    api::PlanError,
    ports::{Fact, IdentityObservation, RejudgementState},
};
use fixtures::{FakePorts, request, selection};

#[test]
fn fr11_hd1_app_host_dd2_recomputes_the_stored_hash_and_rejudges_current_protection() {
    let mut fake = FakePorts::standard();
    let built = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    assert!(built.is_ok());
    let Ok(plan) = built else {
        return;
    };
    fake.save(plan.clone());
    let rejudged = fake.planner().rejudge(&plan.version_id);
    assert!(rejudged.is_ok());
    if let Ok(rejudged) = rejudged {
        assert_eq!(rejudged.recomputed_hash, plan.hash);
        assert_eq!(
            rejudged.ruleset_version,
            plan.ruleset.as_ref().map(|rules| rules.version)
        );
        assert_eq!(
            rejudged.items,
            [(plan.items[0].item_no, RejudgementState::Same)]
        );
    }

    fake.platform.identity = Fact::Known(IdentityObservation::Mismatch);
    let changed = fake.planner().rejudge(&plan.version_id);
    assert!(changed.is_ok());
    if let Ok(changed) = changed {
        assert_eq!(changed.items[0].1, RejudgementState::Denied);
    }
}

#[test]
fn fr11_hd1_app_host_dd2_rejects_a_stored_plan_with_a_changed_canonical_hash() {
    let fake = FakePorts::standard();
    let built = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    assert!(built.is_ok());
    let Ok(mut plan) = built else {
        return;
    };
    plan.expected_effect[0].known_bytes += 1;
    fake.save(plan.clone());
    assert!(matches!(
        fake.planner().rejudge(&plan.version_id),
        Err(PlanError::StoredHashMismatch)
    ));
}

#[test]
fn fr11_hd1_app_host_dd2_returns_undetermined_when_current_ruleset_is_unreadable() {
    let mut fake = FakePorts::standard();
    let built = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1)]));
    assert!(built.is_ok());
    let Ok(plan) = built else {
        return;
    };
    fake.save(plan.clone());
    fake.ruleset = Err(fixtures::port_error("protect.ruleset_unreadable"));
    let rejudged = fake.planner().rejudge(&plan.version_id);
    assert!(rejudged.is_ok());
    if let Ok(rejudged) = rejudged {
        assert_eq!(rejudged.items[0].1, RejudgementState::Undetermined);
        assert_eq!(rejudged.ruleset_version, None);
    }
}
