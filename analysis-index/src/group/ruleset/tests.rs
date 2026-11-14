use super::*;
use crate::query::{Kind, NameEncoding, NameTest};

fn file() -> Condition {
    Condition::Predicate(Predicate::Kind(Kind::File))
}

fn name(value: &str) -> Condition {
    Condition::Predicate(Predicate::Name(
        NameTest::EndsWith(value.as_bytes().to_vec()),
        NameEncoding::Bytes,
    ))
}

fn unknown() -> Condition {
    Condition::Predicate(Predicate::Registered {
        id: "pred.tag".into(),
        revision: 1,
        operand: b"keep".to_vec(),
    })
}

fn draft(name: &str, condition: Condition) -> RuleDraft {
    RuleDraft {
        group_id: None,
        display_name: name.into(),
        condition,
    }
}

fn id(byte: u8) -> Id {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = byte;
    Id::from_v7_bytes(bytes)
}

fn ids() -> impl FnMut() -> Id {
    let mut next = 0u8;
    move || {
        next += 1;
        id(next)
    }
}

fn written(plan: SavePlan) -> Result<Ruleset, String> {
    match plan {
        SavePlan::Write(ruleset) => Ok(ruleset),
        other => Err(format!("expected a write, got {other:?}")),
    }
}

fn redraft(ruleset: &Ruleset) -> Vec<RuleDraft> {
    ruleset
        .rules
        .iter()
        .map(|rule| RuleDraft {
            group_id: Some(rule.group_id.clone()),
            display_name: rule.display_name.clone(),
            condition: rule.condition.clone(),
        })
        .collect()
}

#[test]
fn first_save_assigns_ids_and_version_one() -> Result<(), String> {
    let registry = Registry::baseline();
    let mut next = ids();
    let ruleset = written(plan_save(
        &Ruleset::empty(),
        vec![draft(" Video ", name(".mp4")), draft("Files", file())],
        &registry,
        RuleLimits::default(),
        10,
        &mut next,
    ))?;
    assert_eq!((ruleset.version_no, ruleset.revision), (1, 1));
    assert_eq!(ruleset.rules[0].display_name, "Video");
    assert_ne!(ruleset.rules[0].group_id, ruleset.rules[1].group_id);
    assert_eq!(ruleset.rules[1].predicates, vec![("pred.type".into(), 1)]);
    assert_eq!(ruleset.updated_at_ms, Some(10));
    Ok(())
}

#[test]
fn same_content_is_unchanged_and_order_changes_the_fingerprint() -> Result<(), String> {
    let registry = Registry::baseline();
    let mut next = ids();
    let first = written(plan_save(
        &Ruleset::empty(),
        vec![draft("A", file()), draft("B", name(".log"))],
        &registry,
        RuleLimits::default(),
        1,
        &mut next,
    ))?;
    let again = redraft(&first);
    let unchanged = plan_save(
        &first,
        again.clone(),
        &registry,
        RuleLimits::default(),
        2,
        &mut next,
    );
    assert_eq!(unchanged, SavePlan::Unchanged);
    let mut reordered = again;
    reordered.reverse();
    let second = written(plan_save(
        &first,
        reordered,
        &registry,
        RuleLimits::default(),
        3,
        &mut next,
    ))?;
    assert_eq!((second.version_no, second.revision), (2, 2));
    assert_ne!(second.fingerprint, first.fingerprint);
    assert_eq!(second.rules[0].group_id, first.rules[1].group_id);
    Ok(())
}

#[test]
fn removing_a_group_writes_a_new_version_without_it() -> Result<(), String> {
    let registry = Registry::baseline();
    let mut next = ids();
    let first = written(plan_save(
        &Ruleset::empty(),
        vec![draft("A", file()), draft("B", name(".log"))],
        &registry,
        RuleLimits::default(),
        1,
        &mut next,
    ))?;
    let mut drafts = redraft(&first);
    drafts.remove(0);
    let second = written(plan_save(
        &first,
        drafts,
        &registry,
        RuleLimits::default(),
        2,
        &mut next,
    ))?;
    assert_eq!(second.rules.len(), 1);
    assert_eq!(second.rules[0].group_id, first.rules[1].group_id);
    Ok(())
}

#[test]
fn condition_child_order_does_not_change_the_fingerprint() {
    let a = Rule {
        group_id: id(1),
        display_name: "A".into(),
        condition: Condition::All(vec![file(), name(".a")]),
        predicates: Vec::new(),
    };
    let b = Rule {
        condition: Condition::All(vec![name(".a"), file()]),
        ..a.clone()
    };
    assert_eq!(fingerprint(&[a]), fingerprint(&[b]));
}

#[test]
fn review_reports_the_first_failure_of_each_rule() {
    let registry = Registry::baseline();
    let mut next = ids();
    let limits = RuleLimits {
        max_rules: 4,
        max_name_chars: 4,
    };
    let plan = plan_save(
        &Ruleset::empty(),
        vec![
            draft("ok", file()),
            draft("tag", unknown()),
            draft("OK", file()),
            draft("toolong", file()),
            draft("five", file()),
            RuleDraft {
                group_id: Some(id(9)),
                ..draft("x", file())
            },
        ],
        &registry,
        limits,
        1,
        &mut next,
    );
    let keys = match plan {
        SavePlan::Rejected(issues) => issues
            .iter()
            .map(|issue| (issue.index, issue.reason_key))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    assert_eq!(
        keys,
        vec![
            (1, "predicate.unknown"),
            (2, "group.name_duplicate"),
            (3, "group.name_invalid"),
            (4, "group.rule_limit"),
            (5, "group.rule_limit"),
        ]
    );
}

#[test]
fn unknown_group_ids_are_rejected_and_empty_rulesets_are_unchanged() {
    let registry = Registry::baseline();
    let mut next = ids();
    let plan = plan_save(
        &Ruleset::empty(),
        vec![RuleDraft {
            group_id: Some(id(9)),
            ..draft("x", file())
        }],
        &registry,
        RuleLimits::default(),
        1,
        &mut next,
    );
    assert_eq!(
        plan,
        SavePlan::Rejected(vec![Issue {
            index: 0,
            reason_key: "group.unknown"
        }])
    );
    let empty = plan_save(
        &Ruleset::empty(),
        Vec::new(),
        &registry,
        RuleLimits::default(),
        1,
        &mut next,
    );
    assert_eq!(empty, SavePlan::Unchanged);
}

#[test]
fn stored_rules_with_unregistered_predicates_are_kept_and_marked() {
    let registry = Registry::baseline();
    let rules = vec![
        Rule {
            group_id: id(1),
            display_name: "tag".into(),
            condition: unknown(),
            predicates: strategies(&unknown()),
        },
        Rule {
            group_id: id(2),
            display_name: "files".into(),
            condition: file(),
            predicates: strategies(&file()),
        },
    ];
    assert_eq!(
        review_stored(&registry, &rules),
        vec![RuleReview::PredicateUnregistered, RuleReview::Ok]
    );
}
