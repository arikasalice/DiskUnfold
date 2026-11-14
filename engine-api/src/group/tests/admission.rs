//! Draft review, view channels and duplicate keys of `group.classify`.
use std::sync::{Arc, atomic::AtomicUsize};

use du_core_contracts::{
    group_ipc::GroupCancelOutcome,
    group_rules::{StoredGroupRule, StoredGroupRuleset},
};
use du_job_runtime::JobRuntime;
use serde_json::{Value, json};

use super::{
    Clock, Events, History, Ids, TestResult, at_least, classify, ends_with, harness, id,
    save_video_and_big, tag,
};
use crate::group::channels::Channels;

type Boxed<T> = Result<T, Box<dyn std::error::Error>>;

/// A stored version-1 ruleset; each rule names its single predicate strategy.
pub(super) fn stored(rules: &[(&str, Value, &str)]) -> Boxed<StoredGroupRuleset> {
    let mut rows = Vec::with_capacity(rules.len());
    for (index, (name, condition, predicate)) in rules.iter().enumerate() {
        rows.push(StoredGroupRule {
            group_id: id(40 + u8::try_from(index)?),
            display_name: (*name).to_owned(),
            display_order: u32::try_from(index)?,
            condition_json: serde_json::to_string(condition)?,
            condition_model_version: 1,
            predicates_json: json!([{ "id": predicate, "revision": 1 }]).to_string(),
        });
    }
    Ok(StoredGroupRuleset {
        ruleset_id: id(5),
        format_version: 1,
        version_no: 1,
        revision: 1,
        fingerprint: [0; 32],
        created_at_ms: 1,
        updated_at_ms: 1,
        rules: rows,
    })
}

fn video(name: &str) -> Value {
    json!({ "displayName": name, "condition": ends_with(".mp4") })
}

#[test]
fn drafts_pass_the_checks_of_a_save_before_classification() -> TestResult {
    let harness = harness()?;
    let saved = save_video_and_big(&harness)?;
    let known = saved["rules"][0]["groupId"].clone();
    let too_many: Vec<Value> = (0..65).map(|n| video(&format!("R{n}"))).collect();
    let cases = [
        (json!(too_many), "group.rule_limit"),
        (json!([video(&"가".repeat(65))]), "group.rule_limit"),
        (json!([video("  ")]), "group.rule_limit"),
        (
            json!([video("Video"), video(" video ")]),
            "group.name_duplicate",
        ),
        (
            json!([{ "displayName": "Tagged", "condition": tag() }]),
            "predicate.unknown",
        ),
        (
            json!([{ "groupId": id(9), "displayName": "A", "condition": ends_with(".a") }]),
            "group.unknown",
        ),
        (
            json!([
                { "groupId": known, "displayName": "A", "condition": ends_with(".a") },
                { "groupId": known, "displayName": "B", "condition": ends_with(".b") },
            ]),
            "group.unknown",
        ),
    ];
    for (seq, (rules, reason)) in (1u32..).zip(cases) {
        let (_, error) = harness.call(
            "group.classify",
            json!({ "viewRef": id(7), "requestSeq": seq,
                    "rules": { "kind": "draft", "rules": rules } }),
        )?;
        assert_eq!(error.as_deref(), Some(reason), "case {seq}");
    }
    // Names are limited in characters, and a saved group keeps its identity in a preview.
    let named = classify(
        &harness,
        20,
        json!({ "kind": "draft", "rules": [
            { "groupId": known, "displayName": "가".repeat(64), "condition": ends_with(".mp4") },
        ] }),
    )?;
    assert_eq!(named["summary"]["groups"][0]["groupId"], known);
    let at_limit: Vec<Value> = (0..64).map(|n| video(&format!("R{n}"))).collect();
    let full = classify(&harness, 21, json!({ "kind": "draft", "rules": at_limit }))?;
    assert_eq!(full["summary"]["groups"].as_array().map(Vec::len), Some(64));
    Ok(())
}

#[test]
fn a_later_ruleset_version_with_the_same_content_reports_its_own_version() -> TestResult {
    let harness = harness()?;
    let first = save_video_and_big(&harness)?;
    let ids = [
        first["rules"][0]["groupId"].clone(),
        first["rules"][1]["groupId"].clone(),
    ];
    let one = classify(&harness, 1, json!({ "kind": "saved", "rulesetVersion": 1 }))?;
    for (revision, names) in [(1, ["Clip", "Large"]), (2, ["Video", "Big"])] {
        let saved = harness.ok(
            "group.rules.save",
            json!({ "expectedRevision": revision, "rules": [
                { "groupId": ids[0], "displayName": names[0], "condition": ends_with(".mp4") },
                { "groupId": ids[1], "displayName": names[1], "condition": at_least(10) },
            ] }),
        )?;
        assert_eq!(saved["outcome"], "saved");
    }
    let three = classify(&harness, 2, json!({ "kind": "saved", "rulesetVersion": 3 }))?;
    assert_eq!(
        three["summary"]["rulesetFingerprint"],
        one["summary"]["rulesetFingerprint"]
    );
    assert_eq!(three["summary"]["rulesetVersion"], 3);
    assert_ne!(
        three["summary"]["classificationRef"],
        one["summary"]["classificationRef"]
    );
    Ok(())
}

#[test]
fn member_pages_stop_at_the_end_and_refuse_counts_outside_the_limit() -> TestResult {
    let harness = harness()?;
    let response = classify(
        &harness,
        1,
        json!({ "kind": "draft", "rules": [video("Video")] }),
    )?;
    let reference = response["summary"]["classificationRef"].clone();
    let group = response["summary"]["groups"][0]["groupId"].clone();
    let page = |start: u32, count: u32| {
        harness.call(
            "group.members",
            json!({ "classificationRef": reference, "resultVersion": id(2),
                    "category": { "kind": "group", "groupId": group },
                    "start": start, "count": count }),
        )
    };
    let (last, error) = page(2, 1_000)?;
    assert_eq!(error, None);
    assert_eq!(last["total"], 3);
    assert_eq!(last["rows"].as_array().map(Vec::len), Some(1));
    let (beyond, error) = page(3, 1)?;
    assert_eq!(error, None);
    assert_eq!(beyond["total"], 3);
    assert_eq!(beyond["rows"].as_array().map(Vec::len), Some(0));
    for count in [0, 1_001] {
        let (_, error) = page(0, count)?;
        assert_eq!(
            error.as_deref(),
            Some("cursor.limit_exceeded"),
            "count {count}"
        );
    }
    Ok(())
}

pub(super) fn runtime() -> Boxed<JobRuntime> {
    Ok(JobRuntime::new(
        Arc::new(Ids(AtomicUsize::new(1))),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events),
    )?)
}

fn debug<E: std::fmt::Debug>(error: E) -> String {
    format!("{error:?}")
}

#[test]
fn a_larger_sequence_cancels_only_the_earlier_request_of_its_view() -> TestResult {
    let runtime = runtime()?;
    let channels = Channels::default();
    let first = channels.admit(&id(7), 1, &runtime).map_err(debug)?;
    let other = channels.admit(&id(8), 1, &runtime).map_err(debug)?;
    let second = channels.admit(&id(7), 2, &runtime).map_err(debug)?;
    assert!(first.load(std::sync::atomic::Ordering::Acquire));
    assert!(!other.load(std::sync::atomic::Ordering::Acquire));
    // A delayed older request neither replaces nor cancels the latest one.
    channels.admit(&id(7), 1, &runtime).map_err(debug)?;
    let stale = channels.cancel(&id(7), 1, &runtime).map_err(debug)?;
    assert_eq!(stale, GroupCancelOutcome::NoEffect);
    assert!(!second.load(std::sync::atomic::Ordering::Acquire));
    let latest = channels.cancel(&id(7), 2, &runtime).map_err(debug)?;
    assert_eq!(latest, GroupCancelOutcome::Accepted);
    assert!(second.load(std::sync::atomic::Ordering::Acquire));
    Ok(())
}
