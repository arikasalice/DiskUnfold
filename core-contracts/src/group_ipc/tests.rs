use super::*;
use serde_json::json;

const ID: &str = "0195f856-ab83-7a59-9343-10d29775c9a9";

#[test]
fn classify_request_accepts_saved_and_draft_sources() -> Result<(), serde_json::Error> {
    let saved: GroupClassifyRequest = serde_json::from_value(json!({
        "viewRef": ID, "requestSeq": 3,
        "rules": { "kind": "saved", "rulesetVersion": 2 }
    }))?;
    assert_eq!(saved.rules, GroupRuleSource::Saved { ruleset_version: 2 });
    let draft: GroupClassifyRequest = serde_json::from_value(json!({
        "viewRef": ID, "requestSeq": 4,
        "rules": { "kind": "draft", "rules": [{
            "displayName": "Video",
            "condition": { "kind": "predicate", "predicate": { "kind": "type", "value": "file" } }
        }] }
    }))?;
    assert!(matches!(draft.rules, GroupRuleSource::Draft { ref rules } if rules.len() == 1));
    Ok(())
}

#[test]
fn requests_reject_unknown_fields() {
    let result = serde_json::from_value::<GroupMembersRequest>(json!({
        "classificationRef": ID, "resultVersion": ID,
        "category": { "kind": "overlap" }, "start": 0, "count": 10, "extra": true
    }));
    assert!(result.is_err());
}

#[test]
fn responses_use_tagged_kinds() -> Result<(), serde_json::Error> {
    let response = GroupClassifyResponse::NotApplicable {
        request_seq: 1,
        reason_key: "group.source_unsupported".into(),
    };
    assert_eq!(
        serde_json::to_value(&response)?,
        json!({ "kind": "not_applicable", "requestSeq": 1, "reasonKey": "group.source_unsupported" })
    );
    let rejected = GroupRulesSaveResponse::Rejected {
        reason_key: "group.rule_invalid".into(),
        issues: vec![GroupRuleIssue {
            index: 1,
            reason_key: "group.name_duplicate".into(),
        }],
    };
    assert_eq!(
        serde_json::to_value(&rejected)?,
        json!({ "outcome": "rejected", "reasonKey": "group.rule_invalid",
                "issues": [{ "index": 1, "reasonKey": "group.name_duplicate" }] })
    );
    Ok(())
}
