//! Stored rules whose condition model version this build does not define.
use std::sync::Arc;

use du_core_contracts::group_rules::StoredGroupRuleset;
use serde_json::json;

use super::{MemoryRules, TestResult, admission, ends_with, harness_with, id};

/// A stored version-1 ruleset with two rules; the second carries `model`.
fn with_second_model(model: u32) -> Result<StoredGroupRuleset, Box<dyn std::error::Error>> {
    let mut stored = admission::stored(&[
        ("Video", ends_with(".mp4"), "pred.name"),
        ("Text", ends_with(".txt"), "pred.name"),
    ])?;
    let second = stored.rules.get_mut(1).ok_or("missing rule")?;
    second.condition_model_version = model;
    Ok(stored)
}

#[test]
fn a_rule_condition_model_this_build_does_not_define_refuses_the_whole_ruleset() -> TestResult {
    // A newer model, an older model without a conversion, and zero all count as undefined.
    for model in [2, u32::MAX, 0] {
        let rules = Arc::new(MemoryRules::default());
        let stored = with_second_model(model)?;
        *rules.0.lock().map_err(|_| "poisoned")? = Some(stored.clone());
        let harness = harness_with(rules.clone())?;
        let (_, error) = harness.call("group.rules.get", json!({}))?;
        assert_eq!(
            error.as_deref(),
            Some("db.schema_mismatch"),
            "model {model}"
        );
        let (_, error) = harness.call(
            "group.rules.save",
            json!({ "expectedRevision": 1, "rules": [{ "displayName": "A", "condition": ends_with(".a") }] }),
        )?;
        assert_eq!(
            error.as_deref(),
            Some("db.schema_mismatch"),
            "model {model}"
        );
        for source in [
            json!({ "kind": "saved", "rulesetVersion": 1 }),
            json!({ "kind": "draft", "rules": [{ "displayName": "A", "condition": ends_with(".a") }] }),
        ] {
            let (_, error) = harness.call(
                "group.classify",
                json!({ "viewRef": id(7), "requestSeq": 1, "rules": source }),
            )?;
            assert_eq!(
                error.as_deref(),
                Some("db.schema_mismatch"),
                "model {model}"
            );
        }
        assert_eq!(*rules.0.lock().map_err(|_| "poisoned")?, Some(stored));
    }
    Ok(())
}

#[test]
fn rules_of_the_current_condition_model_still_load() -> TestResult {
    let rules = Arc::new(MemoryRules::default());
    *rules.0.lock().map_err(|_| "poisoned")? = Some(with_second_model(1)?);
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
