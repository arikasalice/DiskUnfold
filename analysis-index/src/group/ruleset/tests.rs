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

/// Two fixed rules as a save stores them; the second pins a registered predicate.
fn golden_rules() -> Vec<Rule> {
    let video = name(".mp4");
    let tagged = Condition::All(vec![file(), unknown()]).normalize();
    vec![
        Rule {
            group_id: id(1),
            display_name: "Video".into(),
            predicates: strategies(&video),
            condition: video,
        },
        Rule {
            group_id: id(2),
            display_name: "Tagged files".into(),
            predicates: strategies(&tagged),
            condition: tagged,
        },
    ]
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Stored rulesets keep this digest, and a save with the same content is recognized
/// by it. The value changes only when the hashed layout (the domain tag,
/// `RULESET_FORMAT_VERSION` or the order and framing of the hashed fields), the
/// condition model version or the canonical condition bytes change. Such a change
/// makes every stored ruleset differ from its own content, so decide that first and
/// update this value second.
const GOLDEN_FINGERPRINT: &str = "9e00b5021e13798cd9af9be2666d2da38b83e2ac140d017e3039dc3390b1612f";

#[test]
fn the_fingerprint_of_a_fixed_ruleset_is_a_fixed_value() {
    let rules = golden_rules();
    assert_eq!(
        rules[1].predicates,
        vec![("pred.tag".into(), 1), ("pred.type".into(), 1)]
    );
    assert_eq!(hex(&fingerprint(&rules)), GOLDEN_FINGERPRINT);
}

/// One change to the fixed ruleset.
type Change = fn(&mut Vec<Rule>);

#[test]
fn the_fingerprint_covers_rule_content_and_order_and_leaves_out_group_ids() {
    let golden = fingerprint(&golden_rules());
    let changed = |change: Change| {
        let mut rules = golden_rules();
        change(&mut rules);
        fingerprint(&rules)
    };
    let variants: [(&str, Change); 4] = [
        ("display name", |rules| {
            rules[0].display_name = "Videos".into()
        }),
        ("condition", |rules| rules[0].condition = name(".mkv")),
        ("predicate revision", |rules| rules[1].predicates[0].1 = 2),
        ("order", |rules| rules.reverse()),
    ];
    let mut seen = vec![golden];
    for (label, change) in variants {
        let value = changed(change);
        assert!(!seen.contains(&value), "{label}");
        seen.push(value);
    }
    let regrouped = changed(|rules| {
        rules[0].group_id = id(8);
        rules[1].group_id = id(9);
    });
    assert_eq!(regrouped, golden);
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
            (3, "group.rule_limit"),
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

#[test]
fn names_lose_surrounding_unicode_white_space_and_keep_inner_space() -> Result<(), String> {
    for (input, stored) in [
        ("  Video", "Video"),
        ("Video  ", "Video"),
        ("\tVideo\t", "Video"),
        ("\u{3000}영상\u{3000}", "영상"),
        ("\u{a0}Video\u{a0}", "Video"),
        ("\u{85}\u{2028}Video\u{202f}\u{205f}", "Video"),
        (" 보고서 초안 ", "보고서 초안"),
        ("Report \u{3000} draft", "Report \u{3000} draft"),
    ] {
        assert_eq!(normalize_name(input), stored, "{input:?}");
    }
    let registry = Registry::baseline();
    let mut next = ids();
    let first = written(plan_save(
        &Ruleset::empty(),
        vec![draft("\u{3000}보고서 초안\t", file())],
        &registry,
        RuleLimits::default(),
        1,
        &mut next,
    ))?;
    assert_eq!(first.rules[0].display_name, "보고서 초안");
    // Other surrounding space around the same name is the same content: nothing is written.
    let mut padded = redraft(&first);
    padded[0].display_name = "\u{a0} 보고서 초안 \u{3000}".into();
    assert_eq!(
        plan_save(
            &first,
            padded,
            &registry,
            RuleLimits::default(),
            2,
            &mut next
        ),
        SavePlan::Unchanged
    );
    Ok(())
}

#[test]
fn blank_and_duplicate_names_are_judged_after_trimming() {
    let registry = Registry::baseline();
    let limits = RuleLimits {
        max_rules: 8,
        max_name_chars: 4,
    };
    let drafts = vec![
        draft("Ab", file()),
        draft("\u{3000}ab\u{a0}", name(".a")),
        draft("", file()),
        draft(" \t\u{3000}\u{a0}", file()),
        // Surrounding space does not count toward the name length limit.
        draft("\t abcd \u{3000}", file()),
        draft("a  b", name(".b")),
    ];
    let issues = review(&Ruleset::empty(), &drafts, &registry, limits);
    assert_eq!(
        issues
            .iter()
            .map(|issue| (issue.index, issue.reason_key))
            .collect::<Vec<_>>(),
        vec![
            (1, "group.name_duplicate"),
            (2, "group.rule_limit"),
            (3, "group.rule_limit"),
        ]
    );
}
