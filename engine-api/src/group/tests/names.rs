//! Group names through `group.rules.save`, `group.rules.get` and a draft `group.classify`:
//! surrounding white space is trimmed before a name is stored, compared or previewed.
use std::sync::Arc;

use serde_json::json;

use super::{MemoryRules, TestResult, admission, at_least, classify, ends_with, harness_with, id};

#[test]
fn stored_returned_and_previewed_names_are_the_trimmed_name() -> TestResult {
    let rules = Arc::new(MemoryRules::default());
    let harness = harness_with(rules.clone())?;
    let saved = harness.ok(
        "group.rules.save",
        json!({ "expectedRevision": 0, "rules": [
            { "displayName": "\u{3000}Report draft\t", "condition": ends_with(".mp4") },
            { "displayName": "\u{a0}Big ", "condition": at_least(10) },
        ] }),
    )?;
    assert_eq!(saved["outcome"], "saved");
    let ruleset = &saved["ruleset"];
    assert_eq!(ruleset["rules"][0]["displayName"], "Report draft");
    assert_eq!(ruleset["rules"][1]["displayName"], "Big");
    let stored: Vec<String> = rules
        .0
        .lock()
        .map_err(|_| "poisoned")?
        .iter()
        .flat_map(|stored| stored.rules.iter().map(|rule| rule.display_name.clone()))
        .collect();
    assert_eq!(stored, ["Report draft", "Big"]);
    assert_eq!(&harness.ok("group.rules.get", json!({}))?, ruleset);

    // The same names with other surrounding space are the same ruleset.
    let again = harness.ok(
        "group.rules.save",
        json!({ "expectedRevision": 1, "rules": [
            { "groupId": ruleset["rules"][0]["groupId"], "displayName": " Report draft ",
              "condition": ends_with(".mp4") },
            { "groupId": ruleset["rules"][1]["groupId"], "displayName": "Big\u{3000}",
              "condition": at_least(10) },
        ] }),
    )?;
    assert_eq!(&again["ruleset"], ruleset);

    let preview = classify(
        &harness,
        1,
        json!({ "kind": "draft", "rules": [
            { "displayName": "\tReport draft\u{3000}", "condition": ends_with(".mp4") },
        ] }),
    )?;
    assert_eq!(
        preview["summary"]["groups"][0]["displayName"],
        "Report draft"
    );
    Ok(())
}

#[test]
fn blank_and_duplicate_names_are_judged_after_trimming_by_save_and_preview() -> TestResult {
    let harness = harness_with(Arc::new(MemoryRules::default()))?;
    let drafts = json!([
        { "displayName": "Video", "condition": ends_with(".mp4") },
        { "displayName": "\u{3000}video\u{a0}", "condition": ends_with(".mkv") },
        { "displayName": " \t\u{3000}\u{a0}", "condition": at_least(10) },
    ]);
    let rejected = harness.ok(
        "group.rules.save",
        json!({ "expectedRevision": 0, "rules": drafts }),
    )?;
    assert_eq!(rejected["outcome"], "rejected");
    assert_eq!(
        rejected["issues"],
        json!([
            { "index": 1, "reasonKey": "group.name_duplicate" },
            { "index": 2, "reasonKey": "group.rule_limit" },
        ])
    );
    for (rules, reason) in [
        (json!([drafts[0], drafts[1]]), "group.name_duplicate"),
        (json!([drafts[2]]), "group.rule_limit"),
    ] {
        let (_, error) = harness.call(
            "group.classify",
            json!({ "viewRef": id(7), "requestSeq": 1,
                    "rules": { "kind": "draft", "rules": rules } }),
        )?;
        assert_eq!(error.as_deref(), Some(reason));
    }
    Ok(())
}

/// Every save trims names, so only another writer could leave surrounding space in a
/// stored row. Such a row is read back as stored; nothing rewrites it on read.
#[test]
fn a_stored_name_with_surrounding_space_is_read_as_stored() -> TestResult {
    let rules = Arc::new(MemoryRules::default());
    *rules.0.lock().map_err(|_| "poisoned")? = Some(admission::stored(&[(
        " Old\u{3000}",
        ends_with(".mp4"),
        "pred.name",
    )])?);
    let harness = harness_with(rules)?;
    let read = harness.ok("group.rules.get", json!({}))?;
    assert_eq!(read["rules"][0]["displayName"], " Old\u{3000}");
    Ok(())
}
