mod fixtures;

use du_change_planner::ports::PlanItemState;
use fixtures::{FakePorts, item_facts, request, selection};

#[test]
fn fr6_hd1_app_host_dd1_keeps_raw_path_first_and_merges_duplicate_identity_evidence() {
    let fake = FakePorts::standard();
    fake.set_item(
        1,
        item_facts(b"/root/z", &[b"root", b"z"], b"same-id", Some(5)),
    );
    fake.set_item(
        2,
        item_facts(b"/root/a", &[b"root", b"a"], b"same-id", Some(5)),
    );
    let plan = fake.planner().build(request(
        &fake,
        vec![selection(&fake, 1), selection(&fake, 2)],
    ));
    assert!(plan.is_ok());
    let Ok(plan) = plan else {
        return;
    };
    assert_eq!(plan.items[0].item.item_no, 2);
    assert_eq!(
        plan.items[0].selection_evidence,
        ["selection.1", "selection.2"]
    );
    assert_eq!(plan.items[1].state, PlanItemState::Rejected);
    assert_eq!(
        plan.items[1].reason_key.as_deref(),
        Some("plan.item.duplicate_target")
    );
    assert_eq!(plan.items[1].absorbed_by, Some(plan.items[0].item_no));
}

#[test]
fn fr6_hd1_app_host_dd1_rejects_duplicate_raw_paths_even_with_distinct_identities() {
    let fake = FakePorts::standard();
    fake.set_item(
        1,
        item_facts(b"/root/same", &[b"root", b"same"], b"file-a", Some(4)),
    );
    fake.set_item(
        2,
        item_facts(b"/root/same", &[b"root", b"same"], b"file-b", Some(8)),
    );
    let plan = fake.planner().build(request(
        &fake,
        vec![selection(&fake, 1), selection(&fake, 2)],
    ));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        assert_eq!(plan.items[0].item.item_no, 1);
        assert_eq!(plan.items[1].state, PlanItemState::Rejected);
        assert_eq!(
            plan.items[1].reason_key.as_deref(),
            Some("plan.item.duplicate_target")
        );
    }
}

#[test]
fn fr6_hd1_app_host_dd1_absorbs_same_condition_descendants() {
    let fake = FakePorts::standard();
    fake.set_item(
        1,
        item_facts(b"/root/a", &[b"root", b"a"], b"parent", Some(8)),
    );
    fake.set_item(
        2,
        item_facts(b"/root/a/b", &[b"root", b"a", b"b"], b"child", Some(3)),
    );
    let plan = fake.planner().build(request(
        &fake,
        vec![selection(&fake, 1), selection(&fake, 2)],
    ));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        assert_eq!(plan.items[0].state, PlanItemState::Included);
        assert_eq!(plan.items[1].state, PlanItemState::Rejected);
        assert_eq!(
            plan.items[1].reason_key.as_deref(),
            Some("plan.item.nested_target")
        );
        assert_eq!(plan.items[1].absorbed_by, Some(plan.items[0].item_no));
    }
}

#[test]
fn fr6_hd1_app_host_dd1_holds_ancestors_of_different_condition_descendants() {
    let fake = FakePorts::standard();
    fake.set_item(
        1,
        item_facts(b"/root/a", &[b"root", b"a"], b"parent", Some(8)),
    );
    fake.set_item(
        2,
        item_facts(b"/root/a/b", &[b"root", b"a", b"b"], b"child", Some(3)),
    );
    let mut child = selection(&fake, 2);
    child.condition = b"different-condition".to_vec();
    let plan = fake
        .planner()
        .build(request(&fake, vec![selection(&fake, 1), child]));
    assert!(plan.is_ok());
    if let Ok(plan) = plan {
        assert_eq!(plan.items[0].state, PlanItemState::Held);
        assert_eq!(
            plan.items[0].reason_key.as_deref(),
            Some("plan.item.overlap_conflict")
        );
        assert_eq!(plan.items[1].state, PlanItemState::Included);
    }
}
