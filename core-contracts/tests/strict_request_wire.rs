//! Pins that the request and stored-rule types kept strict after responses and
//! events became lenient still reject unknown fields and unknown tags. They are
//! decoded from GUI request bodies or from stored F36 group rules.
use du_core_contracts::{
    PathBytes,
    analysis_ipc::{ScopeDraft, ScopeExclusion},
    browse_open_ipc::{QueryCondition, QueryPredicate},
    browse_page_ipc::QueryStrategy,
    visualization::request::{VizViewChannel, VizViewOpenRequest},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, from_value, json, to_value};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// `value` with an unknown field added to the object at `pointer`.
fn extended(value: &Value, pointer: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let mut value = value.clone();
    value
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("no object at {pointer:?}"))?
        .insert("futureField".into(), json!(true));
    Ok(value)
}

/// `valid` decodes and re-encodes unchanged; an unknown field at any of
/// `pointers` is rejected.
fn strict<T: Serialize + DeserializeOwned>(valid: &Value, pointers: &[&str]) -> TestResult {
    assert_eq!(&to_value(from_value::<T>(valid.clone())?)?, valid);
    for pointer in pointers {
        let invalid = extended(valid, pointer)?;
        assert!(from_value::<T>(invalid).is_err(), "{valid} at {pointer:?}");
    }
    Ok(())
}

fn bytes(text: &str) -> Value {
    json!(PathBytes::from_bytes(text.as_bytes()))
}

fn os_path(display: &str) -> Value {
    json!({ "encoding": "unix", "raw": bytes(display), "display": display, "lossy": false })
}

fn predicates() -> [Value; 4] {
    [
        json!({ "kind": "name", "op": "ends_with", "encoding": "bytes", "value": bytes(".mp4") }),
        json!({ "kind": "type", "value": "file" }),
        json!({ "kind": "size", "basis": "logical", "op": "at_least", "bytes": "10" }),
        json!({ "kind": "registered", "id": "pred.tag", "revision": 1, "operand": bytes("keep") }),
    ]
}

#[test]
fn query_strategy_rejects_unknown_fields() -> TestResult {
    strict::<QueryStrategy>(&json!({ "id": "pred.name", "revision": 1 }), &[""])
}

#[test]
fn query_predicate_variants_reject_unknown_fields_and_tags() -> TestResult {
    for predicate in predicates() {
        strict::<QueryPredicate>(&predicate, &[""])?;
    }
    let unknown_tag = json!({ "kind": "future", "value": "file" });
    assert!(from_value::<QueryPredicate>(unknown_tag).is_err());
    Ok(())
}

#[test]
fn query_condition_variants_reject_unknown_fields_and_tags() -> TestResult {
    let [name, kind, ..] = predicates();
    let leaf = json!({ "kind": "predicate", "predicate": name });
    let other = json!({ "kind": "predicate", "predicate": kind });
    strict::<QueryCondition>(&leaf, &["", "/predicate"])?;
    for tag in ["all", "any"] {
        let branch = json!({ "kind": tag, "children": [leaf, other] });
        strict::<QueryCondition>(&branch, &["", "/children/0", "/children/1/predicate"])?;
    }
    let not = json!({ "kind": "not", "child": { "kind": "all", "children": [leaf] } });
    strict::<QueryCondition>(&not, &["", "/child", "/child/children/0/predicate"])?;
    for unknown_tag in [
        json!({ "kind": "xor", "children": [leaf] }),
        json!({ "kind": "not", "child": { "kind": "future", "predicate": name } }),
        json!({ "kind": "predicate", "predicate": { "kind": "future", "value": "file" } }),
    ] {
        assert!(from_value::<QueryCondition>(unknown_tag).is_err());
    }
    Ok(())
}

#[test]
fn scope_exclusion_variants_reject_unknown_fields_and_tags() -> TestResult {
    strict::<ScopeExclusion>(
        &json!({ "kind": "path", "path": os_path("/data/tmp") }),
        &[""],
    )?;
    strict::<ScopeExclusion>(
        &json!({ "kind": "name_pattern", "pattern": "*.tmp" }),
        &[""],
    )?;
    let unknown_tag = json!({ "kind": "glob", "pattern": "*.tmp" });
    assert!(from_value::<ScopeExclusion>(unknown_tag).is_err());
    Ok(())
}

fn scope_draft() -> Value {
    json!({
        "includes": [os_path("/data")],
        "exclusions": [
            { "kind": "path", "path": os_path("/data/tmp") },
            { "kind": "name_pattern", "pattern": "*.tmp" },
        ],
        "boundary": "include_selected",
        "includedMounts": [os_path("/data/mnt")],
        "source": "gui",
    })
}

#[test]
fn scope_draft_rejects_unknown_fields_in_itself_and_its_exclusions() -> TestResult {
    strict::<ScopeDraft>(&scope_draft(), &["", "/exclusions/0", "/exclusions/1"])?;
    let mut unknown_tag = scope_draft();
    unknown_tag["exclusions"][1]["kind"] = json!("glob");
    assert!(from_value::<ScopeDraft>(unknown_tag).is_err());
    Ok(())
}

/// Known gap, pending a protocol decision: `OsPath` is a shared value type that
/// responses also carry, so it accepts unknown fields, and so do the paths inside a
/// `ScopeDraft` request. Flip this test when the request paths become strict.
#[test]
fn scope_draft_paths_still_accept_unknown_fields() -> TestResult {
    for pointer in ["/includes/0", "/includedMounts/0", "/exclusions/0/path"] {
        let accepted = extended(&scope_draft(), pointer)?;
        assert_eq!(
            from_value::<ScopeDraft>(accepted)?,
            from_value(scope_draft())?
        );
    }
    Ok(())
}

#[test]
fn view_lifecycle_requests_reject_unknown_fields() -> TestResult {
    strict::<VizViewOpenRequest>(&json!({}), &[""])?;
    let channel = json!({ "viewChannel": "018f0000-0000-7000-8000-000000000001" });
    strict::<VizViewChannel>(&channel, &[""])
}
