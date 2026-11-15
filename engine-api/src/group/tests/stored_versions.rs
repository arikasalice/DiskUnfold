//! Stored rules whose condition model version this build does not define.
use std::sync::Arc;

use du_core_contracts::group_rules::StoredGroupRuleset;
use serde_json::json;

use super::{MemoryRules, TestResult, admission, ends_with, harness_with, id};

type Boxed<T> = Result<T, Box<dyn std::error::Error>>;

const REFUSED: &str = "db.schema_mismatch";

/// Breaks the stored JSON of the rule at an index.
type Damage = fn(&mut StoredGroupRuleset, usize);

/// A stored version-1 ruleset with two rules of the current condition model.
fn two_rules() -> Boxed<StoredGroupRuleset> {
    admission::stored(&[
        ("Video", ends_with(".mp4"), "pred.name"),
        ("Text", ends_with(".txt"), "pred.name"),
    ])
}

/// `two_rules` with `model` as the condition model of the rules at `indexes`.
fn with_models(indexes: &[usize], model: u32) -> Boxed<StoredGroupRuleset> {
    let mut stored = two_rules()?;
    for &index in indexes {
        let rule = stored.rules.get_mut(index).ok_or("missing rule")?;
        rule.condition_model_version = model;
    }
    Ok(stored)
}

/// Every use case that loads the stored ruleset refuses it, and the stored rows
/// are unchanged afterwards.
fn assert_refused(stored: StoredGroupRuleset, label: &str) -> TestResult {
    let rules = Arc::new(MemoryRules::default());
    *rules.0.lock().map_err(|_| "poisoned")? = Some(stored.clone());
    let harness = harness_with(rules.clone())?;
    let draft = json!([{ "displayName": "A", "condition": ends_with(".a") }]);
    let saved = json!({ "kind": "saved", "rulesetVersion": 1 });
    let calls = [
        ("group.rules.get", json!({})),
        (
            "group.rules.save",
            json!({ "expectedRevision": 1, "rules": draft }),
        ),
        (
            "group.classify",
            json!({ "viewRef": id(7), "requestSeq": 1, "rules": saved }),
        ),
        (
            "group.classify",
            json!({ "viewRef": id(7), "requestSeq": 1,
                    "rules": { "kind": "draft", "rules": draft } }),
        ),
    ];
    for (name, body) in calls {
        let (_, error) = harness.call(name, body)?;
        assert_eq!(error.as_deref(), Some(REFUSED), "{label}: {name}");
    }
    assert_eq!(
        *rules.0.lock().map_err(|_| "poisoned")?,
        Some(stored),
        "{label}"
    );
    Ok(())
}

#[test]
fn a_rule_condition_model_this_build_does_not_define_refuses_the_whole_ruleset() -> TestResult {
    // A newer model, an older model without a conversion, and zero all count as
    // undefined. The other rule of the ruleset has the current model.
    for model in [2, u32::MAX, 0] {
        assert_refused(with_models(&[1], model)?, &format!("second rule, {model}"))?;
        assert_refused(with_models(&[0], model)?, &format!("first rule, {model}"))?;
    }
    Ok(())
}

#[test]
fn a_ruleset_whose_rules_all_have_an_undefined_condition_model_is_refused() -> TestResult {
    for model in [2, u32::MAX, 0] {
        assert_refused(with_models(&[0, 1], model)?, &format!("all rules, {model}"))?;
    }
    Ok(())
}

/// The model is checked for every rule before any rule JSON is parsed: a newer
/// build may also store JSON this build cannot parse, and that must not surface as
/// an internal error or hide the refusal.
#[test]
fn an_undefined_condition_model_is_refused_before_broken_rule_json_is_parsed() -> TestResult {
    let broken: [(&str, Damage); 4] = [
        ("condition is not JSON", |stored, index| {
            stored.rules[index].condition_json = "{".into();
        }),
        ("condition has an unknown node", |stored, index| {
            stored.rules[index].condition_json = json!({ "kind": "future_node" }).to_string();
        }),
        ("predicates are not JSON", |stored, index| {
            stored.rules[index].predicates_json = "not json".into();
        }),
        ("predicates are not a list", |stored, index| {
            stored.rules[index].predicates_json = json!({ "id": 1 }).to_string();
        }),
    ];
    for (label, damage) in broken {
        // The broken row alone reaches the parser and is not a schema mismatch.
        let mut alone = two_rules()?;
        damage(&mut alone, 0);
        let rules = Arc::new(MemoryRules::default());
        *rules.0.lock().map_err(|_| "poisoned")? = Some(alone);
        let (_, error) = harness_with(rules)?.call("group.rules.get", json!({}))?;
        assert!(
            error.as_deref().is_some_and(|key| key != REFUSED),
            "{label}"
        );

        for (broken_row, undefined_row) in [(0, 1), (1, 0)] {
            let mut stored = with_models(&[undefined_row], 2)?;
            damage(&mut stored, broken_row);
            assert_refused(stored, &format!("{label}, broken row {broken_row}"))?;
        }
        // The broken row itself carries the undefined model.
        let mut stored = with_models(&[0], 2)?;
        damage(&mut stored, 0);
        assert_refused(stored, &format!("{label}, same row"))?;
    }
    Ok(())
}

#[test]
fn rules_of_the_current_condition_model_still_load() -> TestResult {
    let rules = Arc::new(MemoryRules::default());
    *rules.0.lock().map_err(|_| "poisoned")? = Some(two_rules()?);
    let harness = harness_with(rules)?;
    let ruleset = harness.ok("group.rules.get", json!({}))?;
    assert_eq!(ruleset["rulesetVersion"], 1);
    let rows: Vec<_> = ruleset["rules"]
        .as_array()
        .ok_or("rules")?
        .iter()
        .map(|rule| {
            (
                rule["displayName"].clone(),
                rule["conditionModelVersion"].clone(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [(json!("Video"), json!(1)), (json!("Text"), json!(1))]
    );
    Ok(())
}
