use super::{
    RulesetBody, UserRuleBody, built_in_body, built_in_v1_row, canonical_ruleset_json_v1,
    hash_ruleset_v1, verify_ruleset_row,
};
use du_core_contracts::ruleset::{
    ClassReleaseState, PROTECTION_RULESET_HASH_FORMAT, RulesetAction, RulesetReadError,
    UserRuleKind, classification_id,
};
use std::collections::BTreeMap;

fn require<T: std::fmt::Debug, E: std::fmt::Debug>(result: Result<T, E>) -> Option<T> {
    assert!(result.is_ok(), "{result:?}");
    result.ok()
}

#[test]
fn built_in_body_encoding_and_hash_are_stable() {
    let Some(first) = require(built_in_v1_row(10)) else {
        return;
    };
    let Some(second) = require(built_in_v1_row(99)) else {
        return;
    };
    assert_eq!(first.body, second.body);
    assert_eq!(first.ruleset_hash, second.ruleset_hash);
    assert_eq!(first.hash_format, PROTECTION_RULESET_HASH_FORMAT);
    assert!(first.body.starts_with("{\"class_actions\":"));
    assert!(first.body.ends_with("\"user_rules\":[]}"));
    assert_eq!(
        verify_ruleset_row(&first).map(|snapshot| snapshot.user_rules.len()),
        Ok(0)
    );
    let Some(snapshot) = require(verify_ruleset_row(&first)) else {
        return;
    };
    assert!(snapshot.class_actions.iter().all(|(id, action)| {
        *action == RulesetAction::Allow
            || snapshot.class_release_state.get(id) == Some(&ClassReleaseState::Protected)
            || snapshot.class_release_state.get(id) == Some(&ClassReleaseState::NotReleasable)
    }));
    assert_eq!(
        snapshot.class_actions.get(classification_id::USER_MANAGED),
        Some(&RulesetAction::Allow)
    );
}

#[test]
fn canonical_json_sorts_user_rules_and_operations_and_uses_minimal_utf8() {
    let rule = |path_bytes: &str, allowed_operations: &[&str], memo: &str| UserRuleBody {
        allowed_operations: allowed_operations
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        kind: UserRuleKind::AllowedOperations,
        memo: memo.into(),
        path_bytes: path_bytes.into(),
        target_file_id: None,
        volume_id: "aa".into(),
    };
    let body = RulesetBody {
        class_actions: BTreeMap::new(),
        class_release_state: BTreeMap::new(),
        user_rules: vec![
            rule("01", &["move"], "plain"),
            rule("02", &["trash", "delete", "delete"], "é"),
        ],
    };
    let Some(encoded) = require(canonical_ruleset_json_v1(body)) else {
        return;
    };
    assert_eq!(
        encoded,
        "{\"class_actions\":{},\"class_release_state\":{},\"user_rules\":[{\"allowed_operations\":[\"delete\",\"trash\"],\"kind\":\"allowed_operations\",\"memo\":\"é\",\"path_bytes\":\"02\",\"target_file_id\":null,\"volume_id\":\"aa\"},{\"allowed_operations\":[\"move\"],\"kind\":\"allowed_operations\",\"memo\":\"plain\",\"path_bytes\":\"01\",\"target_file_id\":null,\"volume_id\":\"aa\"}]}"
    );
}

#[test]
fn canonical_hash_does_not_include_the_hash_format_tag() {
    let Some(row) = require(built_in_v1_row(1)) else {
        return;
    };
    assert_eq!(
        hash_ruleset_v1(row.body.as_bytes()),
        row.ruleset_hash.as_slice()
    );
}

#[test]
fn tampered_body_hash_tag_and_versions_are_rejected() {
    let Some(valid) = require(built_in_v1_row(1)) else {
        return;
    };
    for mutate in [
        |row: &mut du_core_contracts::ruleset::RulesetRow| row.body.push(' '),
        |row: &mut du_core_contracts::ruleset::RulesetRow| row.ruleset_hash[0] ^= 1,
        |row: &mut du_core_contracts::ruleset::RulesetRow| row.hash_format.push_str(".other"),
        |row: &mut du_core_contracts::ruleset::RulesetRow| row.class_table_version = "2".into(),
        |row: &mut du_core_contracts::ruleset::RulesetRow| {
            row.known_location_table_version = "2".into()
        },
        |row: &mut du_core_contracts::ruleset::RulesetRow| row.rule_format_version = 2,
    ] {
        let mut damaged = valid.clone();
        mutate(&mut damaged);
        assert_eq!(verify_ruleset_row(&damaged), Err(RulesetReadError::Corrupt));
    }
    let mut user_rules = valid.clone();
    user_rules.body = user_rules
        .body
        .replace("\"user_rules\":[]", "\"user_rules\":[{}]");
    assert_eq!(
        verify_ruleset_row(&user_rules),
        Err(RulesetReadError::Corrupt)
    );

    let mut added_user_rule = valid;
    added_user_rule.body = added_user_rule.body.replace(
        "\"user_rules\":[]",
        "\"user_rules\":[{\"allowed_operations\":[],\"kind\":\"keep\",\"memo\":\"\",\"path_bytes\":\"01\",\"target_file_id\":null,\"volume_id\":\"aa\"}]",
    );
    added_user_rule.ruleset_hash = hash_ruleset_v1(added_user_rule.body.as_bytes()).to_vec();
    assert_eq!(
        verify_ruleset_row(&added_user_rule),
        Err(RulesetReadError::Corrupt),
        "version one cannot contain user rules even when the hash is recomputed"
    );
    assert_eq!(built_in_body().user_rules.len(), 0);
}
